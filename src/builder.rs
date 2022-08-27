use std::time::Duration;

use super::{Manager, Pool, PoolConfig};

/// Builder for [`Pool`]s.
///
/// Instances of this are created by calling the [`Pool::builder()`] method.
#[must_use = "builder does nothing itself, use `.build()` to build it"]
#[derive(Debug)]
pub struct PoolBuilder<M>
where
    M: Manager,
{
    pub(crate) manager: M,
    pub(crate) config: PoolConfig,
}

impl<M> PoolBuilder<M>
where
    M: Manager,
{
    pub(crate) fn new(manager: M) -> Self {
        Self {
            manager,
            config: PoolConfig::default(),
        }
    }

    /// Builds the [`Pool`].
    ///
    /// # Errors
    ///
    /// See [`BuildError`] for details.
    pub fn build(self) -> Pool<M> {
        Pool::from_builder(self)
    }

    /// Sets a [`PoolConfig`] to build the [`Pool`] with.
    pub fn config(mut self, value: PoolConfig) -> Self {
        self.config = value;
        self
    }

    /// Sets the [`PoolConfig::max_size`].
    pub fn max_size(mut self, value: usize) -> Self {
        self.config.max_size = value;
        self
    }

    /// Sets the [`PoolConfig::timeout`].
    pub fn timeout(mut self, value: Option<Duration>) -> Self {
        self.config.timeout = value;
        self
    }
}
