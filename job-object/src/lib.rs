//! Windows Job Object wrapper for terminating a process and its descendants as a group, plus the
//! token privilege handling needed to create the named Job Objects.
//!
//! The crate is empty on non-windows targets.

#![cfg(windows)]
#![deny(missing_docs)]

mod job_object;
mod privileges;

pub use job_object::{JobObject, JobObjectError};
pub use privileges::{PrivilegeError, SE_CREATE_PERMANENT_PRIVILEGE, enable_privilege};
