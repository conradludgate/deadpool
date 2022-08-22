//! This module contains all things that should be reexported
//! by backend implementations in order to avoid direct
//! dependencies on the `deadpool` crate itself.
//!
//! This module is the variant that should be used by *sync*
//! backends.
//!
//! Crates based on `deadpool::sync` should include this line:
//! ```rust,ignore
//! pub use deadpool::sync::reexports::*;
//! deadpool_reexports!(
//!     "name_of_crate",
//!     Manager,
//!     Object<Manager>,
//!     Error,
//!     ConfigError
//! );
//! ```

pub use super::{InteractError, SyncGuard};
