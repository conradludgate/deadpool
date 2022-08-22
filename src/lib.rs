#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(
    nonstandard_style,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links,
    rustdoc::private_intra_doc_links
)]
#![forbid(non_ascii_idents, unsafe_code)]
#![warn(
    deprecated_in_future,
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    unreachable_pub,
    unused_import_braces,
    unused_labels,
    unused_lifetimes,
    unused_qualifications,
    unused_results
)]

mod builder;
mod config;
// mod dropguard;
mod errors;
// mod hooks;
mod metrics;

/// The current pool status.
#[derive(Clone, Copy, Debug)]
pub struct Status {
    /// The maximum size of the pool.
    pub max_size: usize,

    /// The connections idle in the pool.
    pub size: usize,

    /// The connections available from the pool.
    pub available: usize,
}

use std::{
    fmt,
    future::Future,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::{Arc, Weak},
    time::Duration,
};

use async_trait::async_trait;
use crossbeam_queue::ArrayQueue;
use tokio::{
    sync::{Semaphore, TryAcquireError},
    time::Instant,
};

pub use self::{
    builder::{BuildError, PoolBuilder},
    config::{CreatePoolError, PoolConfig},
    errors::{PoolError, RecycleError, TimeoutType},
    metrics::Metrics,
};

/// Result type of the [`Manager::recycle()`] method.
pub type RecycleResult<E> = Result<(), RecycleError<E>>;

/// Manager responsible for creating new [`Object`]s or recycling existing ones.
#[async_trait]
pub trait Manager: Sync + Send {
    /// Type of [`Object`]s that this [`Manager`] creates and recycles.
    type Type;
    /// Error that this [`Manager`] can return when creating and/or recycling
    /// [`Object`]s.
    type Error;

    /// Creates a new instance of [`Manager::Type`].
    async fn create(&self) -> Result<Self::Type, Self::Error>;

    /// Tries to recycle an instance of [`Manager::Type`].
    ///
    /// # Errors
    ///
    /// Returns [`Manager::Error`] if the instance couldn't be recycled.
    async fn recycle(&self, obj: &mut Self::Type) -> RecycleResult<Self::Error>;

    /// Detaches an instance of [`Manager::Type`] from this [`Manager`].
    ///
    /// This method is called when using the [`Object::take()`] method for
    /// removing an [`Object`] from a [`Pool`]. If the [`Manager`] doesn't hold
    /// any references to the handed out [`Object`]s then the default
    /// implementation can be used which does nothing.
    fn detach(&self, _obj: &mut Self::Type) {}
}

/// Wrapper around the actual pooled object which implements [`Deref`],
/// [`DerefMut`] and [`Drop`] traits.
///
/// Use this object just as if it was of type `T` and upon leaving a scope the
/// [`Drop::drop()`] will take care of returning it to the pool.
#[must_use]
pub struct Object<M: Manager> {
    /// The actual object
    inner: Option<ObjectInner<M>>,

    /// Pool to return the pooled object to.
    pool: Weak<PoolInner<M>>,
}

impl<M> fmt::Debug for Object<M>
where
    M: fmt::Debug + Manager,
    M::Type: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Object")
            .field("inner", &self.inner)
            .finish()
    }
}

struct UnreadyObject<'a, M: Manager> {
    inner: Option<ObjectInner<M>>,
    pool: &'a PoolInner<M>,
}

impl<'a, M: Manager> UnreadyObject<'a, M> {
    fn ready(mut self) -> ObjectInner<M> {
        self.inner.take().unwrap()
    }
}

impl<'a, M: Manager> Drop for UnreadyObject<'a, M> {
    fn drop(&mut self) {
        if let Some(mut inner) = self.inner.take() {
            self.pool.manager.detach(&mut inner.obj);
        }
    }
}

impl<'a, M: Manager> Deref for UnreadyObject<'a, M> {
    type Target = ObjectInner<M>;
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().unwrap()
    }
}

impl<'a, M: Manager> DerefMut for UnreadyObject<'a, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().unwrap()
    }
}

#[derive(Debug)]
pub(crate) struct ObjectInner<M: Manager> {
    /// Actual pooled object.
    obj: M::Type,

    /// Object metrics.
    metrics: Metrics,
}

impl<M: Manager> Object<M> {
    /// Takes this [`Object`] from its [`Pool`] permanently. This reduces the
    /// size of the [`Pool`].
    #[must_use]
    pub fn take(mut this: Self) -> M::Type {
        let mut inner = this.inner.take().unwrap().obj;
        if let Some(pool) = Object::pool(&this) {
            pool.inner.detach_object(&mut inner)
        }
        inner
    }

    /// Get object statistics
    pub fn metrics(this: &Self) -> &Metrics {
        &this.inner.as_ref().unwrap().metrics
    }

    /// Returns the [`Pool`] this [`Object`] belongs to.
    ///
    /// Since [`Object`]s only hold a [`Weak`] reference to the [`Pool`] they
    /// come from, this can fail and return [`None`] instead.
    pub fn pool(this: &Self) -> Option<Pool<M>> {
        this.pool.upgrade().map(|inner| Pool {
            inner,
            _wrapper: PhantomData::default(),
        })
    }
}

impl<M: Manager> Drop for Object<M> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            if let Some(pool) = self.pool.upgrade() {
                pool.return_object(inner)
            }
        }
    }
}

impl<M: Manager> Deref for Object<M> {
    type Target = M::Type;
    fn deref(&self) -> &M::Type {
        &self.inner.as_ref().unwrap().obj
    }
}

impl<M: Manager> DerefMut for Object<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner.as_mut().unwrap().obj
    }
}

impl<M: Manager> AsRef<M::Type> for Object<M> {
    fn as_ref(&self) -> &M::Type {
        self
    }
}

impl<M: Manager> AsMut<M::Type> for Object<M> {
    fn as_mut(&mut self) -> &mut M::Type {
        self
    }
}

/// Generic object and connection pool.
///
/// This struct can be cloned and transferred across thread boundaries and uses
/// reference counting for its internal state.
pub struct Pool<M: Manager, W: From<Object<M>> = Object<M>> {
    inner: Arc<PoolInner<M>>,
    _wrapper: PhantomData<fn() -> W>,
}

// Implemented manually to avoid unnecessary trait bound on `W` type parameter.
impl<M, W> fmt::Debug for Pool<M, W>
where
    M: fmt::Debug + Manager,
    M::Type: fmt::Debug,
    W: From<Object<M>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool")
            .field("inner", &self.inner)
            .field("wrapper", &self._wrapper)
            .finish()
    }
}

impl<M: Manager, W: From<Object<M>>> Clone for Pool<M, W> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _wrapper: PhantomData::default(),
        }
    }
}

impl<M: Manager, W: From<Object<M>>> Pool<M, W> {
    /// Instantiates a builder for a new [`Pool`].
    ///
    /// This is the only way to create a [`Pool`] instance.
    pub fn builder(manager: M) -> PoolBuilder<M, W> {
        PoolBuilder::new(manager)
    }

    pub(crate) fn from_builder(builder: PoolBuilder<M, W>) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                manager: builder.manager,
                slots: Slots {
                    vec: ArrayQueue::new(builder.config.max_size),
                    semaphore: Semaphore::new(builder.config.max_size),
                },
                config: builder.config,
            }),
            _wrapper: PhantomData::default(),
        }
    }

    /// Retrieves an [`Object`] from this [`Pool`] or waits for one to
    /// become available.
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn get(&self) -> Result<W, PoolError<M::Error>> {
        self.timeout_get(self.inner.config.timeout).await
    }

    /// Retrieves an [`Object`] from this [`Pool`] using a different `timeout`
    /// than the configured one.
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn timeout_get(&self, timeouts: Option<Duration>) -> Result<W, PoolError<M::Error>> {
        let non_blocking = match timeouts {
            Some(t) => t.is_zero(),
            None => false,
        };
        let instant = timeouts.and_then(|d| Instant::now().checked_add(d));

        let permit = if non_blocking {
            self.inner
                .slots
                .semaphore
                .try_acquire()
                .map_err(|e| match e {
                    TryAcquireError::Closed => PoolError::Closed,
                    TryAcquireError::NoPermits => PoolError::Timeout(TimeoutType::Wait),
                })?
        } else {
            apply_timeout(TimeoutType::Wait, instant, async {
                self.inner
                    .slots
                    .semaphore
                    .acquire()
                    .await
                    .map_err(|_| PoolError::Closed)
            })
            .await?
        };

        loop {
            let inner_obj = if let Some(inner_obj) = self.inner.slots.vec.pop() {
                self.try_recycle(instant, inner_obj).await?
            } else {
                Some(self.try_create(instant).await?)
            };
            if let Some(inner_obj) = inner_obj {
                permit.forget();

                break Ok(Object {
                    inner: Some(inner_obj),
                    pool: Arc::downgrade(&self.inner),
                }
                .into());
            }
        }
    }

    #[inline]
    async fn try_recycle(
        &self,
        instant: Option<Instant>,
        inner_obj: ObjectInner<M>,
    ) -> Result<Option<ObjectInner<M>>, PoolError<M::Error>> {
        let mut unready_obj = UnreadyObject {
            inner: Some(inner_obj),
            pool: &self.inner,
        };

        if apply_timeout(
            TimeoutType::Recycle,
            instant,
            self.inner.manager.recycle(&mut unready_obj.obj),
        )
        .await
        .is_err()
        {
            return Ok(None);
        }

        Ok(Some(unready_obj.ready()))
    }

    #[inline]
    async fn try_create(
        &self,
        instant: Option<Instant>,
    ) -> Result<ObjectInner<M>, PoolError<M::Error>> {
        Ok(ObjectInner {
            obj: apply_timeout(TimeoutType::Create, instant, self.inner.manager.create()).await?,
            metrics: Metrics::default(),
        })
    }

    /// Closes this [`Pool`].
    ///
    /// All current and future tasks waiting for [`Object`]s will return
    /// [`PoolError::Closed`] immediately.
    ///
    /// This operation resizes the pool to 0.
    pub fn close(&self) {
        self.inner.slots.semaphore.close();
        while self.inner.slots.vec.pop().is_some() {}
    }

    /// Indicates whether this [`Pool`] has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.slots.semaphore.is_closed()
    }

    /// Retrieves [`Status`] of this [`Pool`].
    #[must_use]
    pub fn status(&self) -> Status {
        let size = self.inner.slots.vec.len();
        let max_size = self.inner.slots.vec.capacity();
        let available = self.inner.slots.semaphore.available_permits();
        Status {
            size,
            max_size,
            available,
        }
    }

    /// Returns [`Manager`] of this [`Pool`].
    #[must_use]
    pub fn manager(&self) -> &M {
        &self.inner.manager
    }
}

struct PoolInner<M: Manager> {
    manager: M,
    slots: Slots<ObjectInner<M>>,
    config: PoolConfig,
    // hooks: hooks::Hooks<M>,
}

#[derive(Debug)]
struct Slots<T> {
    vec: ArrayQueue<T>,
    semaphore: Semaphore,
}

// Implemented manually to avoid unnecessary trait bound on the struct.
impl<M> fmt::Debug for PoolInner<M>
where
    M: fmt::Debug + Manager,
    M::Type: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolInner")
            .field("manager", &self.manager)
            .field("slots", &self.slots)
            .field("config", &self.config)
            // .field("hooks", &self.hooks)
            .finish()
    }
}

impl<M: Manager> PoolInner<M> {
    fn return_object(&self, inner: ObjectInner<M>) {
        if let Err(mut inner) = self.slots.vec.push(inner) {
            self.manager.detach(&mut inner.obj);
        } else {
            self.slots.semaphore.add_permits(1);
        }
    }
    fn detach_object(&self, obj: &mut M::Type) {
        self.slots.semaphore.add_permits(1);
        self.manager.detach(obj);
    }
}

async fn apply_timeout<O, E>(
    timeout_type: TimeoutType,
    instant: Option<Instant>,
    future: impl Future<Output = Result<O, impl Into<PoolError<E>>>>,
) -> Result<O, PoolError<E>> {
    match instant {
        None => future.await.map_err(Into::into),
        Some(instant) => tokio::time::timeout_at(instant, future)
            .await
            .map_err(|_| PoolError::Timeout(timeout_type))?
            .map_err(Into::into),
    }
}
