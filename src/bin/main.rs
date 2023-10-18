use std::error::Error;
use std::process;

use tracing::{error, info};

use newrelic_super_agent::agent::instance_id::ULIDInstanceIDGetter;
use newrelic_super_agent::config::resolver::Resolver;
use newrelic_super_agent::opamp::client_builder::OpAMPHttpBuilder;
use newrelic_super_agent::{
    agent::{Agent, AgentEvent},
    cli::Cli,
    cli::running_mode::AgentRunningMode,
    config::agent_type_registry::{AgentRepository, LocalRepository},
    context::Context,
    logging::Logging,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // init logging singleton
    Logging::try_init()?;

    let cli = Cli::init_super_agent_cli();

    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);
        return Ok(());
    }

    // Program must run as root, but should accept simple behaviors such as --version, --help, etc
    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() && cli.get_running_mode() == AgentRunningMode::OnHost {
        return Err("Program must run as root".into());
    }

    info!("Creating the global context");
    let ctx: Context<Option<AgentEvent>> = Context::new();

    info!("Creating the signal handler");
    ctrlc::set_handler({
        let ctx = ctx.clone();
        move || ctx.cancel_all(Some(AgentEvent::Stop)).unwrap()
    })
    .map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    let mut local_agent_type_repository = LocalRepository::new();
    local_agent_type_repository.store_from_yaml(NEWRELIC_INFRA_TYPE.as_bytes())?;
    local_agent_type_repository.store_from_yaml(NRDOT_TYPE.as_bytes())?;

    // load effective config
    let cfg_path = &cli.get_config_path();
    let cfg = Resolver::retrieve_config(cfg_path)?;

    let opamp_client_builder = cfg
        .opamp
        .as_ref()
        .map(|opamp_config| OpAMPHttpBuilder::new(opamp_config.clone()));

    let instance_id_getter = ULIDInstanceIDGetter::default();

    info!("Starting the super agent");
    let agent = Agent::new(
        cfg,
        local_agent_type_repository,
        opamp_client_builder,
        instance_id_getter,
    );

    match agent {
        Ok(agent) => Ok(agent.run(ctx)?),
        Err(e) => {
            error!("agent error: {}", e);
            process::exit(1);
        }
    }
}

const NEWRELIC_INFRA_TYPE: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.0.1
variables:
  config_file:
    description: "Newrelic infra configuration path"
    type: string
    required: false
    default: /etc/newrelic-infra.yml
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${config_file}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay_seconds: 5
"#;

const NRDOT_TYPE: &str = r#"
namespace: newrelic
name: io.opentelemetry.collector
version: 0.0.1
variables:
  config_file:
    description: "Newrelic otel collector configuration path"
    type: string
    required: false
    default: /etc/nr-otel-collector/config.yaml
  otel_exporter_otlp_endpoint:
    description: "Endpoint where NRDOT will send data"
    type: string
    required: false
    default: "otlp.nr-data.net:4317"
  new_relic_memory_limit_mib:
    description: "Memory limit for the NRDOT process"
    type: number
    required: false
    default: 100
deployment:
  on_host:
    executables:
      - path: /usr/bin/nr-otel-collector
        args: "--config=${config_file} --feature-gates=-pkg.translator.prometheus.NormalizeName"
        env: "OTEL_EXPORTER_OTLP_ENDPOINT=${otel_exporter_otlp_endpoint} NEW_RELIC_MEMORY_LIMIT_MIB=${new_relic_memory_limit_mib}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay_seconds: 5
"#;
