use std::time::Duration;

use async_trait::async_trait;
use deadpool::{Manager, Pool};
use itertools::Itertools;
use tokio::time::{sleep, timeout};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Gate {
    Ok,
    Err,
    Slow,
    Never,
}

impl Gate {
    async fn open(&self) -> Result<(), ()> {
        match self {
            Self::Ok => Ok(()),
            Self::Err => Err(()),
            Self::Never => {
                sleep(Duration::MAX).await;
                unreachable!();
            }
            Self::Slow => {
                sleep(Duration::from_nanos(2)).await;
                Ok(())
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct Gates {
    create: Gate,
    recycle: Gate,
}

fn configs() -> impl Iterator<Item = Gates> {
    (0..2)
        .map(|_| &[Gate::Ok, Gate::Err, Gate::Slow, Gate::Never])
        .multi_cartesian_product()
        .map(move |gates| Gates {
            create: *gates[0],
            recycle: *gates[1],
        })
}

fn pools(max_size: usize) -> impl Iterator<Item = Pool<GatedManager>> {
    configs().map(move |gates| {
        let manager = GatedManager { gates };
        Pool::builder(manager).max_size(max_size).build()
    })
}

struct GatedManager {
    gates: Gates,
}

#[async_trait]
impl Manager for GatedManager {
    type Type = ();
    type Error = ();
    async fn create(&self) -> Result<Self::Type, Self::Error> {
        self.gates.create.open().await?;
        Ok(())
    }
    async fn recycle(&self, t: Self::Type) -> Option<Self::Type> {
        self.gates.recycle.open().await.map(|_| t).ok()
    }
}

// This tests various combinations of configurations with
// succeeding, failing, slow and hanging managers and hooks.
// It currently tests 4^5 (=1024) possible combinations and
// therefore takes some time to complete. It is probably not
// neccesary to test all combinations, but doing so doesn't
// hurt either and it is a good stress test of the pool.
#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn test_cancellations() {
    for pool in pools(2) {
        let handles = (0..8)
            .map(|i| {
                let pool = pool.clone();
                tokio::spawn(async move {
                    loop {
                        let _obj = timeout(Duration::from_nanos(i), pool.get()).await;
                        sleep(Duration::from_nanos(i)).await;
                    }
                })
            })
            .collect::<Vec<_>>();
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let status = pool.status();
            assert!(
                status.size <= status.max_size,
                "size({}) > max_size({}), gates: {:?}",
                status.size,
                status.max_size,
                pool.manager().gates
            );
        }
        for handle in &handles {
            handle.abort();
        }
        for handle in handles {
            let _ = handle.await;
        }
        let status = pool.status();
        assert!(
            status.size <= status.max_size,
            "size({}) > max_size({}), gates: {:?}",
            status.size,
            status.max_size,
            pool.manager().gates
        );
        assert!(
            status.available <= status.max_size,
            "available({}) > max_size({}), gates: {:?}",
            status.available,
            status.max_size,
            pool.manager().gates
        );
    }
}
