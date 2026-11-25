//! Linux cgroup based tracking of sub-agent process trees.
//!
//! The crate is empty on non-unix targets. It compiles on any unix to ease development, but
//! cgroupfs operations only work on Linux.

#![cfg(unix)]
#![deny(missing_docs)]

mod detect;
mod error;
mod naming;
mod tracker;

pub use detect::CgroupVersion;
pub use error::{CgroupOperation, CgroupTrackerError, InvalidCgroupName};
pub use naming::CgroupName;
pub use tracker::{CgroupHandle, CgroupTracker};
