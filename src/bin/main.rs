use std::error::Error;

use cfg_if::cfg_if;
use newrelic_super_agent::config::super_agent_configs::SuperAgentConfig;
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

    let super_agent_config =
        SuperAgentConfigLoaderFile::new(&cli.get_config_path()).load_config()?;

    let opamp_client_builder: Option<OpAMPHttpBuilder> = super_agent_config
        .opamp
        .as_ref()
        .map(|opamp_config| OpAMPHttpBuilder::new(opamp_config.clone()));

    let instance_id_getter = ULIDInstanceIDGetter::default();

    Ok(run_super_agent(
        super_agent_config,
        ctx,
        opamp_client_builder,
        instance_id_getter,
    )?)
}

fn run_super_agent(
    config: SuperAgentConfig,
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_client_builder: Option<OpAMPHttpBuilder>,
    instance_id_getter: ULIDInstanceIDGetter,
) -> Result<(), AgentError> {
    cfg_if! {
     if #[cfg(feature = "k8s")] {
            let sub_agent_builder = newrelic_super_agent::sub_agent::k8s::builder::K8sSubAgentBuilder::new(opamp_client_builder.as_ref(), &instance_id_getter);
        } else if #[cfg(feature = "onhost")] {
           let sub_agent_builder = OnHostSubAgentBuilder::new(opamp_client_builder.as_ref(), &instance_id_getter);
        }
    };

    SuperAgent::new(
        LocalEffectiveAgentsAssembler::default(),
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        HashRepositoryFile::default(),
        sub_agent_builder,
    )
    .run(ctx, &config)
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
