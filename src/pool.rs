use std::{
    fmt,
    future::Future,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use tokio::{sync::TryAcquireError, time::Instant};

use crate::{
    metrics::PoolMetrics, Manager, Object, PoolBuilder, PoolConfig, PoolError, Slots, Status,
    TimeoutType,
};

/// Generic object and connection pool.
///
/// This struct can be cloned and transferred across thread boundaries and uses
/// reference counting for its internal state.
pub struct Pool<M: Manager> {
    pub(crate) inner: Arc<PoolInner<M>>,
}

// Implemented manually to avoid unnecessary trait bound on `W` type parameter.
impl<M> fmt::Debug for Pool<M>
where
    M: fmt::Debug + Manager,
    M::Type: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool").field("inner", &self.inner).finish()
    }
}

impl<M: Manager> Clone for Pool<M> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<M: Manager> Pool<M> {
    /// Get the metrics of the pool
    pub fn metrics(&self) -> &PoolMetrics {
        &self.inner.metrics
    }

    /// Instantiates a builder for a new [`Pool`].
    ///
    /// This is the only way to create a [`Pool`] instance.
    pub fn builder(manager: M) -> PoolBuilder<M> {
        PoolBuilder::new(manager)
    }

    pub(crate) fn from_builder(builder: PoolBuilder<M>) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                slots: Slots::new(builder.config.max_size),
                config: builder.config,
                metrics: PoolMetrics::default(),
                manager: builder.manager,
            }),
        }
    }

    /// Retrieves an [`Object`] from this [`Pool`] or waits for one to
    /// become available.
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn get(&self) -> Result<Object<M>, PoolError<M::Error>> {
        self.timeout_get(self.inner.config.timeout).await
    }

    /// Retrieves an [`Object`] from this [`Pool`] using a different `timeout`
    /// than the configured one.
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn timeout_get(
        &self,
        timeouts: Option<Duration>,
    ) -> Result<Object<M>, PoolError<M::Error>> {
        let start = Instant::now();
        let res = self.get_inner(start, timeouts).await;

        self.inner.metrics.record_waiting(start);

        match res {
            Ok(success) => Ok(success),
            Err(error) => {
                let _ = self
                    .inner
                    .metrics
                    .failure_count
                    .fetch_add(1, Ordering::Relaxed);
                Err(error)
            }
        }
    }

    async fn get_inner(
        &self,
        now: Instant,
        timeouts: Option<Duration>,
    ) -> Result<Object<M>, PoolError<M::Error>> {
        let non_blocking = match timeouts {
            Some(t) => t.is_zero(),
            None => false,
        };
        let instant = timeouts.and_then(|d| now.checked_add(d));

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
            let inner_obj = if let Some(inner_obj) = self.inner.slots.vec.pop().await {
                self.try_recycle(instant, inner_obj).await?
            } else {
                Some(self.try_create(instant).await?)
            };
            if let Some(inner_obj) = inner_obj {
                permit.forget();
                break Ok(Object::new(inner_obj, &self.inner));
            }
        }
    }

    #[inline]
    async fn try_recycle(
        &self,
        instant: Option<Instant>,
        inner_obj: M::Type,
    ) -> Result<Option<M::Type>, PoolError<M::Error>> {
        apply_timeout(TimeoutType::Recycle, instant, async move {
            Ok::<_, M::Error>(self.inner.manager.recycle(inner_obj).await)
        })
        .await
    }

    #[inline]
    async fn try_create(&self, instant: Option<Instant>) -> Result<M::Type, PoolError<M::Error>> {
        apply_timeout(TimeoutType::Create, instant, self.inner.manager.create()).await
    }

    /// Closes this [`Pool`].
    ///
    /// All current and future tasks waiting for [`Object`]s will return
    /// [`PoolError::Closed`] immediately.
    ///
    /// This operation resizes the pool to 0.
    pub async fn close(&self) {
        self.inner.slots.semaphore.close();
        while self.inner.slots.vec.pop().await.is_some() {}
    }

    /// Indicates whether this [`Pool`] has been closed.
    #[must_use]
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
            max_size,
            size,
            available,
        }
    }

    /// Returns [`Manager`] of this [`Pool`].
    #[must_use]
    pub fn manager(&self) -> &M {
        &self.inner.manager
    }
}

#[derive(Debug)]
pub(crate) struct PoolInner<M: Manager + ?Sized> {
    pub(crate) slots: Slots<M::Type>,
    config: PoolConfig,
    metrics: PoolMetrics,
    manager: M,
}

impl<M: Manager + ?Sized> PoolInner<M> {
    pub(crate) fn return_object(&self, inner: M::Type, start: Instant) {
        self.metrics.record_active(start);
        if self.slots.vec.push_blocking(inner).is_ok() {
            self.slots.semaphore.add_permits(1);
        }
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
