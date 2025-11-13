//! Entry point for the integration tests.
#![warn(missing_docs)]
#![cfg_attr(target_family = "windows", allow(unused_imports, dead_code))] // to remove once all code paths are covered for windows
mod common;
mod k8s;
mod on_host;
