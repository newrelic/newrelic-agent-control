//! Assorted internal utilities: backoff/retry scheduling, archive extraction, environment-variable
//! loading, privilege detection, thread lifecycle management, time abstractions, and binary metadata.

pub mod backoff_gate;
pub mod binary_metadata;
pub mod env_var;
pub mod extract;
pub mod is_elevated;
#[cfg(target_family = "windows")]
pub mod job_object;
pub mod process_group;
pub mod retry;
pub mod thread_context;
pub mod threads;
pub mod time;

#[cfg(test)]
#[allow(missing_docs)]
pub mod test_runtime;
