//! Windows Job Object wrapper for terminating a process and its descendants as a group, plus the
//! permanent named Job Objects that track sub-agents and the token privilege handling needed to
//! create them.
//!
//! The crate is empty on non-windows targets.

#![cfg(windows)]
#![deny(missing_docs)]

mod job_handle;
mod job_name;
mod permanent;
mod privileges;
mod temporary;

pub use job_handle::{JobAccounting, JobHandle, JobObjectError};
pub use job_name::JobName;
pub use permanent::PermanentJobObject;
pub use privileges::{PrivilegeError, SE_CREATE_PERMANENT_PRIVILEGE, enable_privilege};
pub use temporary::TemporaryJobObject;
