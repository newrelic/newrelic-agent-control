//! Linux cgroup based tracking of sub-agent process trees.
//!
//! The crate is empty on non-unix targets. It compiles on any unix to ease development, but
//! cgroupfs operations only work on Linux.

#![cfg(unix)]
#![deny(missing_docs)]

mod error;
mod hierarchy;
mod naming;
mod tracker;

pub use error::CgroupError;
pub use hierarchy::Hierarchy;
pub use naming::CgroupName;
pub use tracker::CgroupTracker;
