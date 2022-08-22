use std::{fmt, time::Duration};

use super::BuildError;

/// [`Pool`] configuration.
///
/// [`Pool`]: super::Pool
#[derive(Clone, Copy, Debug)]
pub struct PoolConfig {
    /// Maximum size of the [`Pool`].
    ///
    /// [`Pool`]: super::Pool
    pub max_size: usize,

    /// Timeouts of the [`Pool`].
    ///
    /// [`Pool`]: super::Pool
    #[cfg_attr(feature = "serde", serde(default))]
    pub timeout: Option<Duration>,
}

impl PoolConfig {
    /// Creates a new [`PoolConfig`] without any timeouts and with the provided
    /// `max_size`.
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            timeout: None,
        }
    }
}

impl Default for PoolConfig {
    /// Creates a new [`PoolConfig`] with the `max_size` being set to
    /// `cpu_count * 4` ignoring any logical CPUs (Hyper-Threading).
    fn default() -> Self {
        Self::new(num_cpus::get_physical() * 4)
    }
}

/// This error is used when building pools via the config `create_pool`
/// methods.
#[derive(Debug)]
pub enum CreatePoolError<C, B> {
    /// This variant is used for configuration errors
    Config(C),
    /// This variant is used for errors while building the pool
    Build(BuildError<B>),
}

impl<C, B> fmt::Display for CreatePoolError<C, B>
where
    C: fmt::Display,
    B: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(e) => write!(f, "Config: {}", e),
            Self::Build(e) => write!(f, "Build: {}", e),
        }
    }
}

impl<C, B> std::error::Error for CreatePoolError<C, B>
where
    C: std::error::Error,
    B: std::error::Error,
{
}
