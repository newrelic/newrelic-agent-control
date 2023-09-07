use std::error::Error;
use std::process;

use opamp_client::{
    capabilities,
    error::ClientError,
    httpclient::HttpClient,
    opamp::proto::AgentCapabilities,
    operation::settings::StartSettings,
};
use tracing::{error, info};

use newrelic_super_agent::{
    agent::{Agent, AgentEvent},
    cli::Cli,
    config::agent_type_registry::{AgentRepository, LocalRepository},
    context::Context,
    logging::Logging,
};
use newrelic_super_agent::agent::opamp_callbacks::OpampCallbacks;
use newrelic_super_agent::config::effective_config::EffectiveConfigRetriever;
use newrelic_super_agent::config::resolver::Resolver;

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

    // load effective config
    let opamp_url = &cli.get_opamp_url();
    let license_key = &cli.get_license_key();
    let cfg_path = &cli.get_config_path();
    let cfg = Resolver::retrieve_config(cfg_path)?;
    let retriever = EffectiveConfigRetriever::new(cfg_path.to_str().unwrap().to_string());

    // Create OpAMP Client
    let opamp_client = create_http_opamp_client(retriever, opamp_url, license_key)?;
    info!("Starting the super agent");
    let agent = Agent::new(cfg, local_agent_type_repository, opamp_client);

    match agent {
        Ok(agent) => Ok(agent.run(ctx)?),
        Err(e) => {
            error!("agent error: {}", e);
            process::exit(1);
        }
    }
}

fn create_http_opamp_client(
    effective_config_resolver: EffectiveConfigRetriever,
    opamp_url: &String,
    license_key: &String,
) -> Result<HttpClient<EffectiveConfigRetriever, OpampCallbacks>, ClientError>
{
    let headers = [("api-key", license_key.as_str())];
    let url = opamp_url.as_str();
    let start_settings = StartSettings {
        instance_id: "7738XWW0Q98GMAD3NHWZM2PZWZ".to_string(),
        capabilities: capabilities!(AgentCapabilities::ReportsStatus),
    };
    let callbacks = OpampCallbacks::new();


    HttpClient::new(
        effective_config_resolver,
        url,
        headers,
        start_settings,
        callbacks,
    )
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
  sleep:
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
      - path: /bin/sleep
        args: "${sleep}"
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
