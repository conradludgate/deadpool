# Deadpool [![Latest Version](https://img.shields.io/crates/v/deadpool.svg)](https://crates.io/crates/deadpool) [![Build Status](https://img.shields.io/github/workflow/status/bikeshedder/deadpool/Rust)](https://github.com/bikeshedder/deadpool/actions?query=workflow%3ARust) ![Unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg "Unsafe forbidden") [![Rust 1.54+](https://img.shields.io/badge/rustc-1.54+-lightgray.svg "Rust 1.54+")](https://blog.rust-lang.org/2021/07/29/Rust-1.54.0.html)


Deadpool is a dead simple async pool for connections and objects
of any type.

### Example

```rust
use async_trait::async_trait;

#[derive(Debug)]
enum Error { Fail }

struct Computer {}

impl Computer {
    async fn get_answer(&self) -> i32 {
        42
    }
}

struct Manager {}

#[async_trait]
impl deadpool::Manager for Manager {
    type Type = Computer;
    type Error = Error;
    
    async fn create(&self) -> Result<Computer, Error> {
        Ok(Computer {})
    }
    
    async fn recycle(&self, c: Computer) -> Option<Computer> {
        Some(c)
    }
}

type Pool = deadpool::Pool<Manager>;

#[tokio::main]
async fn main() {
    let mgr = Manager {};
    let pool = Pool::builder(mgr).build();
    let mut conn = pool.get().await.unwrap();
    let answer = conn.get_answer().await;
    assert_eq!(answer, 42);
}
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0)>
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT)>

at your option.
