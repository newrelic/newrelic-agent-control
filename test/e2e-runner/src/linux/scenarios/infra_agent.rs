use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::nrql::Region;
use crate::common::on_drop::CleanUp;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData};
use crate::{
    common::nrql,
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use std::path::Path;
use std::time::Duration;
use tracing::info;

const SHARED_FILESYSTEM_DIR: &str = "/var/lib/newrelic-agent-control/shared-filesystem";
const SHARED_OHI_BINARIES_DIR: &str = "infra-agent-ohi-binaries";
const SHARED_OHI_CONFIGS_DIR: &str = "infra-agent-ohi-configs";
const EMBEDDED_OHI_BINARIES: [&str; 3] = ["nri-flex", "nri-docker", "nri-prometheus"];

pub fn test_installation_with_infra_agent(args: InstallationArgs) {
    let infra_version = args
        .infra_agent_version
        .clone()
        .expect("--infra-agent-version is required for this scenario");

    let staging = args.nr_region == Region::Staging;

    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    let infra_agent_id: &str = "nr-infra";

    info!("Setup Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path(infra_agent_id),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  staging: {staging}
config_logging:
    logging.yml:
      logs:
      - name: syslog
        file: /var/log/syslog
        attributes:
          host.id: {test_id}
version: {}
"#,
            infra_version
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(
        nrql = nrql_query,
        "Checking results of NRQL to check SystemSample"
    );
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    let nrql_query = format!(r#"SELECT * FROM Log WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL to check logs");
    let retries = 30;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Checking embedded OHI binaries and configs were copied to the shared filesystem");
    retry_panic(
        30,
        Duration::from_secs(2),
        "shared filesystem OHI binaries and configs",
        check_ohi_shared_filesystem,
    );

    info!("Test completed successfully");
}

fn check_ohi_shared_filesystem() -> TestResult<()> {
    let binaries_dir = Path::new(SHARED_FILESYSTEM_DIR).join(SHARED_OHI_BINARIES_DIR);
    for binary in EMBEDDED_OHI_BINARIES {
        let path = binaries_dir.join(binary);
        if !path.is_file() {
            return Err(format!("expected OHI binary not found at {}", path.display()).into());
        }
    }

    let ohi_configs_path = Path::new(SHARED_FILESYSTEM_DIR).join(SHARED_OHI_CONFIGS_DIR);
    if !ohi_configs_path.is_dir() {
        return Err(format!(
            "expected OHI config not found at {}",
            ohi_configs_path.display()
        )
        .into());
    }

    Ok(())
}
