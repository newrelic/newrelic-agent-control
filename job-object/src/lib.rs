//! Windows Job Object wrapper for terminating a process and its descendants as a group.
//!
//! The crate is empty on non-windows targets.

#![cfg(windows)]
#![deny(missing_docs)]

mod job_object;

pub use job_object::{JobObject, JobObjectError};
