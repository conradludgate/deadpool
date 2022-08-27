use std::fmt::Display;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tokio::task::JoinHandle;

//const ITERATIONS: usize = 1_048_576;
const ITERATIONS: usize = 1 << 15;

struct Manager;

#[async_trait::async_trait]
impl deadpool::Manager for Manager {
    type Type = ();
    type Error = ();
    async fn create(&self) -> Result<Self::Type, Self::Error> {
        Ok(())
    }
    async fn recycle(&self, t: Self::Type) -> Option<Self::Type> {
        Some(t)
    }
}

type Pool = deadpool::Pool<Manager>;

#[derive(Copy, Clone, Debug)]
struct Config {
    pool_size: usize,
    workers: usize,
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "w{}s{}", self.workers, self.pool_size)
    }
}

impl Config {
    fn operations_per_worker(&self) -> usize {
        ITERATIONS / self.workers
    }

    async fn run(self, pool: Pool) {
        for _ in 0..self.operations_per_worker() {
            let _ = pool.get().await;
        }
    }
}

#[rustfmt::skip]
const CONFIGS: &[Config] = &[
    // 8 workers
    Config { workers:  8, pool_size:  2 },
    Config { workers:  8, pool_size:  4 },
    Config { workers:  8, pool_size:  8 },
    // 16 workers
    Config { workers: 16, pool_size:  4 },
    Config { workers: 16, pool_size:  8 },
    Config { workers: 16, pool_size: 16 },
    // 32 workers
    Config { workers: 32, pool_size:  8 },
    Config { workers: 32, pool_size: 16 },
    Config { workers: 32, pool_size: 32 },
];

fn bench_get(cfg: Config) {
    let pool = Pool::builder(Manager).max_size(cfg.pool_size).build();

    std::thread::scope(|s| {
        for _ in 0..cfg.workers {
            s.spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap();

                runtime.block_on(cfg.run(pool.clone()));
            });
        }
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("managed");
    for &config in CONFIGS {
        group.bench_function(BenchmarkId::new("get", config), |b| {
            b.iter(|| bench_get(config))
        });
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
