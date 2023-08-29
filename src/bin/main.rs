use newrelic_super_agent::{
    agent::{Agent, AgentEvent},
    cli::Cli,
    config::agent_type_registry::{AgentRepository, LocalRepository},
    context::Context,
    logging::Logging,
};
use std::error::Error;
use tracing::{error, info};

fn main() -> Result<(), Box<dyn Error>> {
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
    if !nix::unistd::Uid::effective().is_root() {
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

    info!("Starting the super agent");
    Ok(Agent::new(&cli.get_config_path(), local_agent_type_repository)?.run(ctx)?)
}

const NEWRELIC_INFRA_TYPE: &str = r#"
name: newrelic-infra
namespace: newrelic
version: 1.39.1
spec:
  config:
    description: "Newrelic infra configuration yaml"
    type: file
    required: true
meta:
  deployment:
    on_host:
      executables:
        - path: /opt/homebrew/bin/newrelic-infra
          args: "--config ${config}"
          env: "NRIA_DISPLAY_NAME=infra_agent_1_1"
        - path: /opt/homebrew/bin/newrelic-infra
          args: "--config ${config}"
          env: "NRIA_DISPLAY_NAME=infra_agent_1_2"
"#;

const NEWRELIC_INFRA_USER_CONFIG: &str = r#"
config: | 
    license: abc123
    staging: true
"#;

const NRDOT_USER_CONFIG: &str = r#"
deployment:
  on_host:
    path: "/etc"
    args: --verbose true
"#;

const NRDOT_TYPE: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
spec:
  deployment:
    on_host:
      path:
        description: "Path to the agent"
        type: string
        required: true
      args:
        description: "Args passed to the agent"
        type: string
        required: true
meta:
  deployment:
    on_host:
      executables:
        - path: ${deployment.on_host.path}/otelcol
          args: "-c ${deployment.on_host.args}"
          env: ""
"#;
