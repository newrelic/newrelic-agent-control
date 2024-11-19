//! OpAMP HTTP client implementation.

#[cfg(feature = "async-http")]
pub mod r#async;
#[cfg(feature = "async-http")]
pub use r#async::*;

#[cfg(feature = "sync-http")]
pub mod sync;
#[cfg(feature = "sync-http")]
pub use sync::*;

pub mod config;
pub use config::HttpConfig;
