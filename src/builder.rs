use std::{fmt, marker::PhantomData, time::Duration};

use super::{Manager, Object, Pool, PoolConfig};

/// Possible errors returned when [`PoolBuilder::build()`] fails to build a
/// [`Pool`].
#[derive(Debug)]
pub enum BuildError<E> {
    /// Backend reported an error when creating a [`Pool`].
    Backend(E),

    /// [`Runtime`] is required.
    NoRuntimeSpecified(String),
}

impl<E: std::fmt::Display> fmt::Display for BuildError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(e) => write!(f, "Error occurred while building the pool: Backend: {}", e),
            Self::NoRuntimeSpecified(msg) => write!(
                f,
                "Error occurred while building the pool: NoRuntimeSpecified: {}",
                msg
            ),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BuildError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(e) => Some(e),
            Self::NoRuntimeSpecified(_) => None,
        }
    }
}

/// Builder for [`Pool`]s.
///
/// Instances of this are created by calling the [`Pool::builder()`] method.
#[must_use = "builder does nothing itself, use `.build()` to build it"]
pub struct PoolBuilder<M, W = Object<M>>
where
    M: Manager,
    W: From<Object<M>>,
{
    pub(crate) manager: M,
    pub(crate) config: PoolConfig,
    _wrapper: PhantomData<fn() -> W>,
}

// Implemented manually to avoid unnecessary trait bound on `W` type parameter.
impl<M, W> fmt::Debug for PoolBuilder<M, W>
where
    M: fmt::Debug + Manager,
    W: From<Object<M>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolBuilder")
            .field("manager", &self.manager)
            .field("config", &self.config)
            .finish()
    }
}

impl<M, W> PoolBuilder<M, W>
where
    M: Manager,
    W: From<Object<M>>,
{
    pub(crate) fn new(manager: M) -> Self {
        Self {
            manager,
            config: PoolConfig::default(),
            _wrapper: PhantomData::default(),
        }
    }

    /// Builds the [`Pool`].
    ///
    /// # Errors
    ///
    /// See [`BuildError`] for details.
    pub fn build(self) -> Pool<M, W> {
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

    /// Sets the [`PoolConfig::timeouts`].
    pub fn timeout(mut self, value: Option<Duration>) -> Self {
        self.config.timeout = value;
        self
    }
}
