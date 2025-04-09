use crate::agent_control::run::Environment;

pub(crate) const RUST_VERSION: &str = env!("CARGO_PKG_RUST_VERSION");
pub(crate) const VERSION: &str =
    konst::option::unwrap_or!(option_env!("AGENT_CONTROL_VERSION"), "development");
pub(crate) const GIT_COMMIT: &str =
    konst::option::unwrap_or!(option_env!("GIT_COMMIT"), "development");

pub fn binary_metadata(env: Environment) -> String {
    format!("New Relic Agent Control ({env}) Version: {VERSION}, Rust Version: {RUST_VERSION}, GitCommit: {GIT_COMMIT}")
}
