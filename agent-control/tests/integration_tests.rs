//! Entry point for the integration tests.
#![warn(missing_docs)]

#[expect(dead_code)] // As some common helpers are not used in windows tests.
mod common;
#[cfg(target_family = "unix")]
mod k8s;
mod on_host;
