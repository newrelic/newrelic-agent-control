use crate::common::RecipeData;
use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::docker_hub::{latest_published_ac_tag, latest_published_infra_agent_tag};
use crate::common::nrql::Region;
use crate::common::oci::OciRegistry;
use crate::common::ohi_agent::{
    AGENT_TYPE_REPOSITORY, OhiAgentArgs, POLL_INTERVAL, check_assertion, dump_status,
    mirror_infra_agent, package_media_type, push_agent_package, push_agent_type,
    render_agent_local_config, run_id, substitute_test_id,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::{STATUS_RUNNING, restart_service};
use std::fs::read_to_string;
use tracing::info;

const SUB_AGENT_ID: &str = "agent-under-test";

pub fn test_ohi_agent(args: OhiAgentArgs) {
    let run_id = run_id();
    info!(run_id, "Starting the OHI agent scenario");

    // Everything that can be checked without installing anything is checked first.
    let owner_config = read_to_string(&args.config_file)
        .unwrap_or_else(|err| panic!("could not read '{}': {err}", args.config_file.display()));
    let agent_config = render_agent_local_config(&owner_config, &run_id)
        .unwrap_or_else(|err| panic!("invalid config file: {err}"));
    let media_type = package_media_type(&args.package_file)
        .unwrap_or_else(|err| panic!("invalid package: {err}"));
    let nrql_query = substitute_test_id(&args.nrql_assertion, &run_id)
        .unwrap_or_else(|err| panic!("invalid NRQL assertion: {err}"));

    let infra_agent_version = latest_published_infra_agent_tag()
        .unwrap_or_else(|err| panic!("could not resolve the latest infra agent version: {err}"));
    info!(infra_agent_version, "Infra agent version resolved");

    let agent_control_version = latest_published_ac_tag()
        .unwrap_or_else(|err| panic!("could not resolve the latest Agent Control version: {err}"));
    info!(agent_control_version, "Agent Control version resolved");

    let installation_args = args.installation_args(agent_control_version);
    let staging = args.nr_region == Region::Staging;

    let _clean_up = CleanUp::new(|| {
        tear_down_test();
        dump_status();
    });
    install_agent_control_from_recipe(&RecipeData {
        args: installation_args.clone(),
        ..Default::default()
    });

    let registry = OciRegistry::start();
    let package_reference = push_agent_package(&registry, &args.package_file, media_type, &run_id);
    info!(%package_reference, "Agent package pushed");
    let (agent_type_reference, agent_type_meta) =
        push_agent_type(&registry, &args.agent_type_file, &run_id)
            .unwrap_or_else(|err| panic!("could not push the agent type: {err}"));
    info!(%agent_type_reference, "Agent type pushed");
    let infra_agent_reference = mirror_infra_agent(&registry, &infra_agent_version)
        .unwrap_or_else(|err| panic!("could not mirror the infra agent package: {err}"));
    info!(%infra_agent_reference, "Infra agent package mirrored");

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {run_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
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
        &windows::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  staging: {staging}
  custom_attributes:
    test.id: {run_id}
version: {infra_agent_version}
"#
        ),
    );
    write_agent_local_config(&windows::local_config_path(SUB_AGENT_ID), agent_config);

    restart_service(SERVICE_NAME, STATUS_RUNNING);

    info!(nrql = nrql_query, "Waiting for the assertion to pass");
    retry_panic(args.poll_retries(), POLL_INTERVAL, "NRQL assertion", || {
        check_assertion(&installation_args, &nrql_query)
    });

    info!("OHI agent scenario completed successfully");
}
