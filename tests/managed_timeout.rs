use std::{convert::Infallible, time::Duration};

use async_trait::async_trait;

use deadpool::{Object, PoolConfig, PoolError, RecycleResult};

type Pool = deadpool::Pool<Manager, Object<Manager>>;

struct Manager {}

#[async_trait]
impl deadpool::Manager for Manager {
    type Type = usize;
    type Error = Infallible;

    async fn create(&self) -> Result<usize, Infallible> {
        std::future::pending().await
    }

    async fn recycle(&self, _conn: &mut usize) -> RecycleResult<Infallible> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn test_managed_timeout() {
    let mgr = Manager {};
    let cfg = PoolConfig {
        max_size: 16,
        timeout: Some(Duration::from_millis(0)),
    };
    let pool = Pool::builder(mgr).config(cfg).build();

    assert!(matches!(pool.get().await, Err(PoolError::Timeout(_))));
}
