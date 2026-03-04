use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::agent_control::run::Environment;

pub(crate) const RUST_VERSION: &str = env!("CARGO_PKG_RUST_VERSION");
pub(crate) const GIT_COMMIT: &str = env!("GIT_COMMIT");

pub fn binary_metadata(env: Environment) -> String {
    format!(
        "New Relic Agent Control Version: {AGENT_CONTROL_VERSION}, Rust Version: {RUST_VERSION}, GitCommit: {GIT_COMMIT}, Environment: {env}"
    )
}
