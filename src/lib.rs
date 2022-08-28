#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(
    nonstandard_style,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links,
    rustdoc::private_intra_doc_links
)]
#![warn(clippy::pedantic)]
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
#![allow(
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::match_same_arms
)]

mod array_queue;
mod builder;
mod config;
mod errors;
mod metrics;
mod object;
mod pool;

pub use self::{
    builder::PoolBuilder,
    config::PoolConfig,
    errors::{PoolError, TimeoutType},
    metrics::PoolMetrics,
    object::Object,
    pool::Pool,
};

use array_queue::ArrayQueue;
use async_trait::async_trait;
use tokio::sync::Semaphore;

/// The current pool status.
#[derive(Clone, Copy, Debug)]
pub struct Status {
    /// The maximum size of the pool.
    pub max_size: usize,

    /// The current items idle in the pool.
    pub size: usize,

    /// The permits available from the pool.
    pub available: usize,
}

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
    /// Returns [`None`] if the instance couldn't be recycled.
    async fn recycle(&self, obj: Self::Type) -> Option<Self::Type>;
}

#[derive(Debug)]
struct Slots<T> {
    vec: ArrayQueue<T>,
    semaphore: Semaphore,
}

impl<T> Slots<T> {
    pub(crate) fn new(max_size: usize) -> Self {
        Self {
            vec: ArrayQueue::new(max_size),
            semaphore: Semaphore::new(max_size),
        }
    }
}
