use std::fmt;

/// Possible steps causing the timeout in an error returned by [`Pool::get()`]
/// method.
///
/// [`Pool::get()`]: super::Pool::get
#[derive(Clone, Copy, Debug)]
pub enum TimeoutType {
    /// Timeout happened while waiting for a slot to become available.
    Wait,

    /// Timeout happened while creating a new object.
    Create,

    /// Timeout happened while recycling an object.
    Recycle,
}

/// Possible errors returned by [`Pool::get()`] method.
///
/// [`Pool::get()`]: super::Pool::get
#[derive(Debug)]
pub enum PoolError<E> {
    /// Timeout happened.
    Timeout(TimeoutType),

    /// Backend reported an error.
    Backend(E),

    /// [`Pool`] has been closed.
    ///
    /// [`Pool`]: super::Pool
    Closed,
}

impl<E> From<E> for PoolError<E> {
    fn from(e: E) -> Self {
        Self::Backend(e)
    }
}

impl<E: fmt::Display> fmt::Display for PoolError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout(tt) => match tt {
                TimeoutType::Wait => write!(
                    f,
                    "Timeout occurred while waiting for a slot to become available"
                ),
                TimeoutType::Create => write!(f, "Timeout occurred while creating a new object"),
                TimeoutType::Recycle => write!(f, "Timeout occurred while recycling an object"),
            },
            Self::Backend(e) => write!(f, "Error occurred while creating a new object: {}", e),
            Self::Closed => write!(f, "Pool has been closed"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PoolError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Timeout(_) | Self::Closed => None,
            Self::Backend(e) => Some(e),
        }
    }
}
