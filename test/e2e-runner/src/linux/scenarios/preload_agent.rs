use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::{InstallationArgs, RecipeData};
use crate::{
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
        bash::exec_bash_command
    },
};
use tracing::{debug, info};

pub fn test_installation_with_preload_agent(args: InstallationArgs) {
    let preload_version = args
        .preload_version
        .clone()
        .expect("--preload-agent-version is required for this scenario");

    let staging = matches!(args.nr_region.to_lowercase().as_str(), "staging");

    let recipe_data = RecipeData {
        args,
        monitoring_source: "preload-agent".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-preload-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    let preload_agent_id = "nr-preload";

    info!("Setup Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-preload:
    agent_type: "newrelic/com.newrelic.preload:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path(preload_agent_id),
        // Correct Config?
        format! {r#"
fleet_id: alphanumeric_id # needed anymore?
apm_language: java
agent_version: 8.13.0
application_names:
  - my-app
  - functions
  - lib
  - bin
new_relic_license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
staging: {staging}
version: {preload_version}"#},
    );


    // ToDo update with actual path
    let ld_preload_path = "path_to_ld_preload";
    let install_command = format!(r#"echo "{ld_preload_path}" >> /ec/ld.so.preload"#);
    let output = exec_bash_command(&install_command)
        .unwrap_or_else(|err| panic!("Editing /ec/ld.so.preload failed: {err}"));
    debug!("echo output:\n{output}");

    linux::service::restart_service(linux::SERVICE_NAME);

    let ls_command = format!(r#"ls {ld_preload_path}"#);
    let output = exec_bash_command(&ls_command)
        .unwrap_or_else(|err| panic!("Installation failed: {err}"));
    debug!("ls output:\n{output}");

    info!("Test completed successfully");
}
