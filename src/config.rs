use std::time::Duration;

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
