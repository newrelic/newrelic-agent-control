use crate::super_agent::defaults::{
    FQN_NAME_INFRA_AGENT, FQN_NAME_NRDOT, NEWRELIC_INFRA_AGENT_VERSION, NR_OTEL_COLLECTOR_VERSION,
};

pub(crate) const VERSION: &str =
    konst::option::unwrap_or!(option_env!("SUPER_AGENT_VERSION"), "development");
pub(crate) const RUST_VERSION: &str =
    konst::option::unwrap_or!(option_env!("RUST_VERSION"), "development");
pub(crate) const GIT_COMMIT: &str =
    konst::option::unwrap_or!(option_env!("GIT_COMMIT"), "development");
pub(crate) const BUILD_DATE: &str =
    konst::option::unwrap_or!(option_env!("BUILD_DATE"), "1970-01-01");

pub fn binary_metadata() -> String {
    format!("New Relic Super Agent Version: {VERSION}, Rust Version: {RUST_VERSION}, GitCommit: {GIT_COMMIT}, BuildDate: {BUILD_DATE}")
}
pub fn sub_agent_versions() -> String {
    format!(
        r#"New Relic Sub Agent Versions:
    {FQN_NAME_INFRA_AGENT} : {NEWRELIC_INFRA_AGENT_VERSION}
    {FQN_NAME_NRDOT} : {NR_OTEL_COLLECTOR_VERSION}"#
    )
}
