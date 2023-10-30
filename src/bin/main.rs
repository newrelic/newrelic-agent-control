use std::error::Error;
use std::path::PathBuf;

use newrelic_super_agent::sub_agent::on_host::builder::OnHostSubAgentBuilder;
use newrelic_super_agent::super_agent::error::AgentError;
use tracing::{error, info};

use newrelic_super_agent::config::loader::{SuperAgentConfigLoader, SuperAgentConfigLoaderFile};
use newrelic_super_agent::config::remote_config_hash::HashRepositoryFile;
use newrelic_super_agent::opamp::client_builder::OpAMPHttpBuilder;
use newrelic_super_agent::super_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use newrelic_super_agent::super_agent::instance_id::ULIDInstanceIDGetter;
use newrelic_super_agent::super_agent::super_agent::{SuperAgent, SuperAgentEvent};
use newrelic_super_agent::{
    cli::running_mode::AgentRunningMode, cli::Cli, context::Context, logging::Logging,
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

    // Program must run as root if running_mode=OnHost, but should accept simple behaviors such as --version, --help, etc
    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() && cli.running_mode() == AgentRunningMode::OnHost {
        return Err("Program must run as root".into());
    }

    info!("Creating the global context");
    let ctx: Context<Option<SuperAgentEvent>> = Context::new();

    info!("Creating the signal handler");
    create_shutdown_signal_handler(ctx.clone())?;

    Ok(run_super_agent(cli.get_config_path(), ctx)?)
}

#[cfg(feature = "onhost")]
fn run_super_agent(
    config_path: PathBuf,
    ctx: Context<Option<SuperAgentEvent>>,
) -> Result<(), AgentError> {
    // load effective config
    let super_agent_config = SuperAgentConfigLoaderFile::new(&config_path).load_config()?;

    let effective_agents_asssembler = LocalEffectiveAgentsAssembler::default();

    let opamp_client_builder: Option<OpAMPHttpBuilder> = super_agent_config
        .opamp
        .as_ref()
        .map(|opamp_config| OpAMPHttpBuilder::new(opamp_config.clone()));

    let instance_id_getter = ULIDInstanceIDGetter::default();

    info!("Starting the super agent");
    SuperAgent::new(
        effective_agents_asssembler,
        opamp_client_builder.as_ref(),
        ULIDInstanceIDGetter::default(),
        HashRepositoryFile::default(),
        OnHostSubAgentBuilder::new(opamp_client_builder.as_ref(), &instance_id_getter),
    )
    .run(ctx, &super_agent_config)
}

#[cfg(feature = "k8s")]
fn run_super_agent(
    config_path: PathBuf,
    ctx: Context<Option<SuperAgentEvent>>,
) -> Result<(), AgentError> {
    // load effective config
    let super_agent_config = SuperAgentConfigLoaderFile::new(&config_path).load_config()?;

    let effective_agents_asssembler = LocalEffectiveAgentsAssembler::default();

    let opamp_client_builder: Option<OpAMPHttpBuilder> = super_agent_config
        .opamp
        .as_ref()
        .map(|opamp_config| OpAMPHttpBuilder::new(opamp_config.clone()));

    let instance_id_getter = ULIDInstanceIDGetter::default();

    info!("Starting the super agent");
    Ok(SuperAgent::new(
        effective_agents_asssembler,
        opamp_client_builder.as_ref(),
        ULIDInstanceIDGetter::default(),
        HashRepositoryFile::default(),
        newrelic_super_agent::sub_agent::k8s::builder::K8sSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
        ),
    )
    .run(ctx, &super_agent_config)?)
}

fn create_shutdown_signal_handler(
    ctx: Context<Option<SuperAgentEvent>>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || ctx.cancel_all(Some(SuperAgentEvent::Stop)).unwrap()).map_err(
        |e| {
            error!("Could not set signal handler: {}", e);
            e
        },
    )?;

    Ok(())
}
