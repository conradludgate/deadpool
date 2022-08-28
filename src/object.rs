use std::{
    fmt,
    ops::{Deref, DerefMut},
    sync::{Arc, Weak},
};

use tokio::time::Instant;

use crate::{pool::PoolInner, Manager, Pool};

/// Wrapper around the actual pooled object which implements [`Deref`],
/// [`DerefMut`] and [`Drop`] traits.
///
/// Use this object just as if it was of type `T` and upon leaving a scope the
/// [`Drop::drop()`] will take care of returning it to the pool.
#[must_use]
pub struct Object<M: Manager + ?Sized> {
    /// The actual object
    inner: Option<M::Type>,

    /// Pool to return the pooled object to.
    pool: Weak<PoolInner<M>>,

    /// Time this object was claimed
    start: Instant,
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

// #[derive(Debug)]
// pub(crate) struct ObjectInner<Type> {
//     /// Actual pooled object.
//     pub(crate) obj: Type,

//     /// Object metrics.
//     pub(crate) metrics: ObjectMetrics,
// }

impl<M: Manager> Object<M> {
    pub(crate) fn new(inner: M::Type, pool: &Arc<PoolInner<M>>) -> Self {
        Self {
            inner: Some(inner),
            pool: Arc::downgrade(pool),
            start: Instant::now(),
        }
    }

    /// Takes this [`Object`] from its [`Pool`] permanently. This reduces the
    /// size of the [`Pool`].
    #[must_use]
    pub fn take(mut this: Self) -> M::Type {
        let inner = this.inner.take().unwrap();
        if let Some(pool) = Object::pool(&this) {
            pool.inner.slots.semaphore.add_permits(1);
        }
        inner
    }

    // /// Get object statistics
    // pub fn metrics(this: &Self) -> &ObjectMetrics {
    //     &this.inner.as_ref().unwrap().metrics
    // }

    /// Returns the [`Pool`] this [`Object`] belongs to.
    ///
    /// Since [`Object`]s only hold a [`Weak`] reference to the [`Pool`] they
    /// come from, this can fail and return [`None`] instead.
    pub fn pool(this: &Self) -> Option<Pool<M>> {
        this.pool.upgrade().map(|inner| Pool { inner })
    }
}

impl<M: Manager + ?Sized> Drop for Object<M> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            if let Some(pool) = self.pool.upgrade() {
                pool.return_object(inner, self.start);
            }
        }
    }
}

impl<M: Manager> Deref for Object<M> {
    type Target = M::Type;
    fn deref(&self) -> &M::Type {
        self.inner.as_ref().unwrap()
    }
}

impl<M: Manager> DerefMut for Object<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().unwrap()
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
