use crate::common::RecipeData;
use crate::common::agent_metadata::{AgentKind, AgentMetadata};
use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::health::dump_status;
use crate::common::nrql::{Region, check_query_results_are_not_empty};
use crate::common::oci::OciRegistry;
use crate::common::ohi_agent::{
    AGENT_TYPE_REPOSITORY, NRQL_ASSERTION_RETRIES, OhiAgentArgs, POLL_INTERVAL, mirror_agent_type,
    mirror_infra_agent, package_media_type, push_agent_package, push_agent_type,
    render_agent_local_config, substitute_test_id, test_id,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_latest_agent_control, tear_down_test};
use crate::windows::service::{STATUS_RUNNING, restart_service};
use std::fs::read_to_string;
use tracing::info;

const SUB_AGENT_ID: &str = "agent-under-test";

pub fn test_ohi_agent(args: OhiAgentArgs) {
    let test_id = test_id();
    info!(test_id, "Starting the OHI agent scenario");

    // Verify and expand inputs on configs and artifacts.
    let input_config = read_to_string(&args.config_file)
        .unwrap_or_else(|err| panic!("could not read '{}': {err}", args.config_file.display()));
    let agent_config = render_agent_local_config(&input_config, &test_id);
    let media_type = package_media_type(&args.package_file);
    let nrql_query = substitute_test_id(&args.nrql_assertion, &test_id);

    // Runs on exit, also when the scenario panics.
    let _clean_up = CleanUp::new(|| {
        tear_down_test();
        dump_status(super::DEFAULT_STATUS_PORT);
    });

    // Installs the latest released AC with local config only, as a customer would.
    let AgentMetadata {
        sub_agent_id: infra_sub_agent_id,
        agent_type_id: infra_agent_type,
        package_version: infra_agent_version,
    } = args.agent(AgentKind::InfraAgent);
    let installation_args = args.installation_args();
    let staging = args.nr_region == Region::Staging;
    install_latest_agent_control(&RecipeData {
        args: installation_args.clone(),
        ..Default::default()
    });

    // Prepare Local registry serving the packages and the agent type to AC.
    let registry = OciRegistry::start();
    let package_reference = push_agent_package(&registry, &args.package_file, media_type, &test_id);
    info!(%package_reference, "Agent package pushed");
    let (agent_type_reference, agent_type_meta) =
        push_agent_type(&registry, &args.agent_type_file, &test_id);
    info!(%agent_type_reference, "Agent type pushed");

    // As the mirror config forces to fetch all artifacts from there we need to mirror the infra-agent package and type as well.
    let infra_agent_reference = mirror_infra_agent(&registry, &infra_agent_version);
    info!(%infra_agent_reference, "Infra agent package mirrored");
    let infra_agent_type_reference =
        mirror_agent_type(&registry, &infra_agent_type, "host-windows");
    info!(%infra_agent_type_reference, "Infra agent type mirrored");

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  {infra_sub_agent_id}:
    agent_type: "{infra_agent_type}"
  {SUB_AGENT_ID}:
    agent_type: "{}"
oci:
  registry: {}
agent_packages:
  signature_verification_enabled: false
agent_types:
  default_remote:
    repository: {AGENT_TYPE_REPOSITORY}
    signature_verification_enabled: false
{DEBUG_LOGGING_CONFIG}
"#,
            agent_type_meta.id(),
            registry.url(),
        ),
    );

    write_agent_local_config(
        &windows::local_config_path(infra_sub_agent_id),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  staging: {staging}
  custom_attributes:
    test.id: {test_id}
version: {infra_agent_version}
"#
        ),
    );

    write_agent_local_config(&windows::local_config_path(SUB_AGENT_ID), agent_config);

    // Restart so AC loads the new configuration.
    restart_service(SERVICE_NAME, STATUS_RUNNING);

    info!(nrql = nrql_query, "Waiting for the NRQL assertion to pass");
    retry_panic(
        NRQL_ASSERTION_RETRIES,
        POLL_INTERVAL,
        "NRQL assertion",
        || check_query_results_are_not_empty(&installation_args, &nrql_query),
    );

    info!("OHI agent scenario completed successfully");
}
