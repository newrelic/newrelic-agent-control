use std::{fs, time::Duration};

use tracing::{debug, info};

use crate::{
    linux::{
        self,
        bash::exec_bash_command,
        install::{Args, RecipeData, install_agent_control_from_recipe},
        service,
    },
    tools::{config, nrql, test::retry},
};

pub fn test_migration(args: Args) {
    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setting up the infra-agent to report some mysql data");
    install_and_setup_infra_agent_with_mysql(&args, test_id.as_str());

    info!("Checking that nri-mysql is reporting data");
    let nrql_query = format!(
        r#"SELECT * FROM MysqlSample WHERE label.test_id = '{test_id}' AND label.test_installed_agent = 'infra_agent' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 12;
    retry(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&args, &nrql_query)
    })
    .unwrap_or_else(|err| {
        panic!("query '{nrql_query}' failed after {retries} retries: {err}");
    });

    info!("Installing Agent Control that will automatically run migration");
    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);
    info!("Changing the 'test_installed_agent' label to from 'infra_agent' to 'agent_control'");
    config::replace_string_in_file(
        "/etc/newrelic-agent-control/local-data/nr-infra/local_config.yaml",
        "test_installed_agent: infra_agent",
        "test_installed_agent: agent_control",
    );

    service::restart_service(linux::SERVICE_NAME);

    info!("Check that nri-mysql is reporting data with the config provided by Agent Control");
    let nrql_query = format!(
        r#"SELECT * FROM MysqlSample WHERE label.test_id = '{test_id}' AND label.test_installed_agent = 'agent_control' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 12;
    retry(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    })
    .unwrap_or_else(|err| {
        panic!("query '{nrql_query}' failed after {retries} retries: {err}");
    });
}

pub fn install_and_setup_infra_agent_with_mysql(args: &Args, test_id: &str) {
    // Install infra agent
    let install_command = format!(
        r#"
curl -Ls https://download.newrelic.com/install/newrelic-cli/scripts/install.sh | \
  bash && sudo \
  NEW_RELIC_CLI_SKIP_CORE=1 \
  NEW_RELIC_LICENSE_KEY={} \
  NEW_RELIC_API_KEY={} \
  NEW_RELIC_ACCOUNT_ID={} \
  NEW_RELIC_REGION={} \
  /usr/local/bin/newrelic install -n infrastructure-agent-installer"#,
        args.nr_license_key,
        args.nr_api_key,
        args.nr_account_id,
        args.nr_region.to_uppercase(),
    );
    info!("Executing recipe to install the Infrastructure Agent");
    let output = retry(3, Duration::from_secs(30), "recipe installation", || {
        exec_bash_command(&install_command)
    })
    .unwrap_or_else(|err| panic!("failure executing recipe after retries: {err}"));
    debug!("Output:\n{output}");

    // Update infra-agent configuration
    fs::write(
        "/etc/newrelic-infra.yml",
        format!(
            r#"
enable_process_metrics: true
status_server_enabled: true
status_server_port: 18003
license_key: {}
    "#,
            args.nr_license_key
        ),
    )
    .unwrap_or_else(|err| {
        panic!("Error updating infra-agent config: {err}");
    });

    // Run a mysql service and install the nri-mysql integration
    let docker_command =
        "docker run -d --rm --name some-mysql -p 3306:3306 -e MYSQL_ROOT_PASSWORD=root -d mysql:8";
    info!(command = docker_command, "Running mysql service");
    exec_bash_command(docker_command).unwrap_or_else(|err| {
        panic!("Could not start the docker mysql service: {err}");
    });

    info!("Installing nri-mysql integration");
    // Install nri-mysql integration
    exec_bash_command("apt install nri-mysql -y").unwrap_or_else(|err| {
        panic!("Could not install nri-mysql: {err}");
    });

    fs::write(
        "/etc/newrelic-infra/integrations.d/nri-mysql-config.yml",
        format!(
            r#"
integrations:
  - name: nri-mysql
    labels:
      test_id: {test_id}
      test_installed_agent: infra_agent
    env:
      HOSTNAME: "localhost"
      PORT: 3306
      USERNAME: "root"
      PASSWORD: "root"
      REMOTE_MONITORING: true
    interval: "20s"
    inventory_source: config/mysql
"#
        ),
    )
    .unwrap_or_else(|err| {
        panic!("Error writing nri-mysql config: {err}");
    });

    service::restart_service("newrelic-infra");
}
