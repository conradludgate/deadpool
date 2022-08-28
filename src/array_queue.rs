//! The implementation is based on Dmitry Vyukov's bounded MPMC queue. Implemented by the crossbeam project
//!
//! Source:
//!   - <http://www.1024cores.net/home/lock-free-algorithms/queues/bounded-mpmc-queue>
//!   - <https://github.com/crossbeam-rs/crossbeam/blob/70700182c4c92c393fc76209c7acad7af69dca21/crossbeam-queue/src/array_queue.rs>
//!
//! The MIT License (MIT)
//! Copyright (c) 2019 The Crossbeam Project Developers
//!
//! Permission is hereby granted, free of charge, to any
//! person obtaining a copy of this software and associated
//! documentation files (the "Software"), to deal in the
//! Software without restriction, including without
//! limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of
//! the Software, and to permit persons to whom the Software
//! is furnished to do so, subject to the following
//! conditions:
//!
//! The above copyright notice and this permission notice
//! shall be included in all copies or substantial portions
//! of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
//! ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
//! TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
//! PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
//! SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//! CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
//! OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
//! IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.

use std::boxed::Box;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{self, AtomicUsize, Ordering};
use std::{fmt, hint, thread};

use crossbeam_utils::CachePadded;
use tokio::task::yield_now;

/// A slot in a queue.
struct Slot<T> {
    /// The current stamp.
    ///
    /// If the stamp equals the tail, this node will be next written to. If it equals head + 1,
    /// this node will be next read from.
    stamp: AtomicUsize,

    /// The value in this slot.
    value: UnsafeCell<MaybeUninit<T>>,
}

pub(crate) struct ArrayQueue<T> {
    /// The head of the queue.
    ///
    /// This value is a "stamp" consisting of an index into the buffer and a lap, but packed into a
    /// single `usize`. The lower bits represent the index, while the upper bits represent the lap.
    ///
    /// Elements are popped from the head of the queue.
    head: CachePadded<AtomicUsize>,

    /// The tail of the queue.
    ///
    /// This value is a "stamp" consisting of an index into the buffer and a lap, but packed into a
    /// single `usize`. The lower bits represent the index, while the upper bits represent the lap.
    ///
    /// Elements are pushed into the tail of the queue.
    tail: CachePadded<AtomicUsize>,

    /// The buffer holding slots.
    buffer: Box<[Slot<T>]>,

    /// The queue capacity.
    cap: usize,

    /// A stamp with the value of `{ lap: 1, index: 0 }`.
    one_lap: usize,
}

unsafe impl<T: Send> Sync for ArrayQueue<T> {}
unsafe impl<T: Send> Send for ArrayQueue<T> {}

enum Pause {
    Spin,
    Yield,
}

enum Flow<B, C> {
    Break(B),
    Continue(Pause, Option<usize>, C),
}

impl<T> ArrayQueue<T> {
    pub(crate) fn new(cap: usize) -> ArrayQueue<T> {
        assert!(cap > 0, "capacity must be non-zero");

        // Head is initialized to `{ lap: 0, index: 0 }`.
        // Tail is initialized to `{ lap: 0, index: 0 }`.
        let head = 0;
        let tail = 0;

        // Allocate a buffer of `cap` slots initialized
        // with stamps.
        let buffer: Box<[Slot<T>]> = (0..cap)
            .map(|i| {
                // Set the stamp to `{ lap: 0, index: i }`.
                Slot {
                    stamp: AtomicUsize::new(i),
                    value: UnsafeCell::new(MaybeUninit::uninit()),
                }
            })
            .collect();

        // One lap is the smallest power of two greater than `cap`.
        let one_lap = (cap + 1).next_power_of_two();

        ArrayQueue {
            buffer,
            cap,
            one_lap,
            head: CachePadded::new(AtomicUsize::new(head)),
            tail: CachePadded::new(AtomicUsize::new(tail)),
        }
    }

    fn index(&self, stamp: usize) -> (usize, usize) {
        // Deconstruct the tail.
        let index = stamp & (self.one_lap - 1);
        let lap = stamp & !(self.one_lap - 1);

        let new = if index + 1 < self.cap {
            // Same lap, incremented index.
            // Set to `{ lap: lap, index: index + 1 }`.
            stamp + 1
        } else {
            // One lap forward, index wraps around to zero.
            // Set to `{ lap: lap.wrapping_add(1), index: 0 }`.
            lap.wrapping_add(self.one_lap)
        };
        (index, new)
    }

    #[inline]
    fn try_push(&self, tail: usize, value: T) -> Flow<Result<(), T>, T> {
        let (index, new_tail) = self.index(tail);

        // Inspect the corresponding slot.
        debug_assert!(index < self.buffer.len());
        let slot = unsafe { self.buffer.get_unchecked(index) };

        let stamp = slot.stamp.load(Ordering::Acquire);

        // If the tail and the stamp match, we may attempt to push.
        if tail == stamp {
            // Try moving the tail.
            match self.tail.compare_exchange_weak(
                tail,
                new_tail,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                // We updated the tail before any other thread - this slot is ours!
                Ok(_) => {
                    // Write the value into the slot and update the stamp.
                    unsafe {
                        slot.value.get().write(MaybeUninit::new(value));
                    }
                    // Mark the slot as init by setting the stamp index is offset from the buffer index
                    slot.stamp.store(tail + 1, Ordering::Release);
                    Flow::Break(Ok(()))
                }
                // We lost the race - another thread just acquired this slot before we could
                Err(t) => Flow::Continue(Pause::Spin, Some(t), value),
            }
        } else if stamp.wrapping_add(self.one_lap) == tail + 1 {
            // this slot seems to be the head of the queue.

            // synchronise with the tail store from above
            atomic::fence(Ordering::SeqCst);

            let head = self.head.load(Ordering::Relaxed);
            if head.wrapping_add(self.one_lap) == tail {
                // ...then the queue is full.
                return Flow::Break(Err(value));
            }

            // if the head updated, then there's likely a new spot available
            Flow::Continue(Pause::Spin, None, value)
        } else {
            // Another thread already acquired this slot and is busy writing,
            // but hasn't yet updated the stamp
            Flow::Continue(Pause::Yield, None, value)
        }
    }

    pub(crate) fn push_blocking(&self, mut value: T) -> Result<(), T> {
        let mut step = 0;
        let mut tail = self.tail.load(Ordering::Relaxed);

        loop {
            match self.try_push(tail, value) {
                Flow::Break(b) => break b,
                Flow::Continue(pause, t, v) => {
                    value = v;
                    // pause the thread - hints that the thread should sync
                    match pause {
                        Pause::Yield if step > 6 => thread::yield_now(),
                        _ => hint::spin_loop(),
                    }
                    step += 1;
                    tail = t.unwrap_or_else(|| self.tail.load(Ordering::Relaxed));
                }
            }
        }
    }

    #[inline]
    fn try_pop(&self, head: usize) -> Flow<Option<T>, ()> {
        let (index, new_head) = self.index(head);

        // Inspect the corresponding slot.
        debug_assert!(index < self.buffer.len());
        let slot = unsafe { self.buffer.get_unchecked(index) };
        let stamp = slot.stamp.load(Ordering::Acquire);

        // If the the stamp is ahead of the head by 1, we may attempt to pop.
        if head + 1 == stamp {
            // Try moving the head.
            match self.head.compare_exchange_weak(
                head,
                new_head,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                // We updated the head before any other thread - this slot is ours!
                Ok(_) => {
                    // Read the value from the slot and update the stamp.
                    let msg = unsafe { slot.value.get().read().assume_init() };
                    // Mark the slot as uninit by setting the stamp index equal to the buffer index
                    slot.stamp
                        .store(head.wrapping_add(self.one_lap), Ordering::Release);
                    Flow::Break(Some(msg))
                }
                // We lost the race - another thread just acquired this slot before we could
                Err(h) => Flow::Continue(Pause::Spin, Some(h), ()),
            }
        } else if stamp == head {
            // this slot is uninit

            atomic::fence(Ordering::SeqCst);
            let tail = self.tail.load(Ordering::Relaxed);

            // If the tail equals the head, that means the channel is empty.
            if tail == head {
                return Flow::Break(None);
            }

            Flow::Continue(Pause::Spin, None, ())
        } else {
            Flow::Continue(Pause::Yield, None, ())
        }
    }

    pub(crate) async fn pop(&self) -> Option<T> {
        let mut step = 0;
        let mut head = self.head.load(Ordering::Relaxed);

        loop {
            match self.try_pop(head) {
                Flow::Break(b) => break b,
                Flow::Continue(pause, h, ()) => {
                    // pause the thread - hints that the thread should sync
                    match pause {
                        Pause::Yield if step > 6 => yield_now().await,
                        _ => hint::spin_loop(),
                    }
                    step += 1;
                    head = h.unwrap_or_else(|| self.head.load(Ordering::Relaxed));
                }
            }
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        self.cap
    }

    pub(crate) fn len(&self) -> usize {
        loop {
            // Load the tail, then load the head.
            let tail = self.tail.load(Ordering::SeqCst);
            let head = self.head.load(Ordering::SeqCst);

            // If the tail didn't change, we've got consistent values to work with.
            if self.tail.load(Ordering::SeqCst) == tail {
                break self.len_impl(head, tail);
            }
        }
    }

    #[inline]
    fn len_impl(&self, head: usize, tail: usize) -> usize {
        let head_index = head & (self.one_lap - 1);
        let tail_index = tail & (self.one_lap - 1);

        if head_index < tail_index {
            tail_index - head_index
        } else if head_index > tail_index {
            self.cap - head_index + tail_index
        } else if tail == head {
            0
        } else {
            self.cap
        }
    }
}

impl<T> Drop for ArrayQueue<T> {
    fn drop(&mut self) {
        // Get the index of the head.
        let head = *self.head.get_mut();
        let tail = *self.tail.get_mut();

        let len = self.len_impl(head, tail);
        let head_index = head & (self.one_lap - 1);

        // Loop over all slots that hold a message and drop them.
        for i in 0..len {
            // Compute the index of the next slot holding a message.
            let index = if head_index + i < self.cap {
                head_index + i
            } else {
                head_index + i - self.cap
            };

            unsafe {
                debug_assert!(index < self.buffer.len());
                let slot = self.buffer.get_unchecked_mut(index);
                let value = &mut *slot.value.get();
                value.as_mut_ptr().drop_in_place();
            }
        }
    }
}

impl<T> fmt::Debug for ArrayQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("ArrayQueue { .. }")
    }
}
