use newrelic_super_agent::{
    agent::{Agent, AgentEvent},
    cli::Cli,
    config::agent_type_registry::{AgentRepository, LocalRepository},
    context::Context,
    logging::Logging,
};
use std::error::Error;
use std::process;
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
    local_agent_type_repository.store_from_yaml(RANDOM_CMDS_TYPE.as_bytes())?;

    info!("Starting the super agent");
    let agent = Agent::new(&cli.get_config_path(), local_agent_type_repository);

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
name: newrelic-infra
version: 1.39.1
variables:
  config:
    description: "Newrelic infra configuration yaml"
    type: file
    required: true
deployment:
  on_host:
    executables:
      - path: /opt/homebrew/bin/newrelic-infra
        args: "--config ${config}"
        env: "NRIA_DISPLAY_NAME=infra_agent_1_1"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay_seconds: 5
        max_retries: 5
        last_retry_interval_seconds: 60
      restart_exit_codes: [1, 2]
"#;

const RANDOM_CMDS_TYPE: &str = r#"
namespace: davidsanchez
name: random-commands
version: 0.0.1
variables:
  ip:
    description: "Destination IP to make pings"
    type: string
    required: true
  message:
    description: "Content to output with 'echo'"
    type: string
    required: false
    default: "Supervisor!"
deployment:
  on_host:
    executables:
      - path: /Users/davidsanchez/.nix-profile/bin/ping
        args: "${ip}"
      - path: echo
        args: "Hello ${message}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay_seconds: 1
        max_retries: 0
        last_retry_interval_seconds: 60
"#;

const _NRDOT_TYPE: &str = r#"
namespace: newrelic
name: nrdot
version: 0.1.0
variables:
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
deployment:
  on_host:
    executables:
      - path: ${deployment.on_host.path}/otelcol
        args: "-c ${deployment.on_host.args}"
        env: ""
"#;
