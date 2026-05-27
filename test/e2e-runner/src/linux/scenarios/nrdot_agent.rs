use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::install::tear_down_test;
use crate::{
    common::nrql,
    linux::{self, install::install_agent_control_from_recipe},
};
use std::net::UdpSocket;
use std::time::Duration;
use tracing::info;

pub fn test_nrdot_agent(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        monitoring_source: "discovery".to_string(),
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);
    let test_id = format!(
        "onhost-e2e-discovery-flow_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setup Agent Control config with discovery flow monitoring agent");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nrdot:
    agent_type: newrelic/com.newrelic.opentelemetry.collector:0.1.0
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path("nrdot"),
        r#"
agent_dir: "/tmp"
"#
        .to_string(),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    info!("Waiting for model to be executed to start before sending synthetic flow data");
    std::thread::sleep(Duration::from_secs(90));
}
