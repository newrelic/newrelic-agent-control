//! Builds a human-readable string describing the running binary (version, Rust version, git commit,
//! and environment).

use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::environment::Environment;

pub(crate) const RUST_VERSION: &str = env!("CARGO_PKG_RUST_VERSION");
pub(crate) const GIT_COMMIT: &str = env!("GIT_COMMIT");

/// Returns a one-line summary of the binary's version, Rust version, git commit, and `env`.
pub fn binary_metadata(env: Environment) -> String {
    format!(
        "New Relic Agent Control Version: {AGENT_CONTROL_VERSION}, Rust Version: {RUST_VERSION}, GitCommit: {GIT_COMMIT}, Environment: {env}"
    )
}
