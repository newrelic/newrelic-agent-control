use std::error::Error;

use newrelic_super_agent::super_agent::error::AgentError;
use tracing::{error, info};

use newrelic_super_agent::config::remote_config_hash::HashRepositoryFile;
use newrelic_super_agent::config::store::{SuperAgentConfigStore, SuperAgentConfigStoreFile};
use newrelic_super_agent::opamp::client_builder::OpAMPHttpBuilder;
use newrelic_super_agent::super_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use newrelic_super_agent::super_agent::instance_id::ULIDInstanceIDGetter;
use newrelic_super_agent::super_agent::super_agent::{SuperAgent, SuperAgentEvent};
use newrelic_super_agent::{cli::Cli, context::Context, logging::Logging};

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

    info!("Creating the global context");
    let ctx: Context<Option<SuperAgentEvent>> = Context::new();

    info!("Creating the signal handler");
    create_shutdown_signal_handler(ctx.clone())?;

    let mut super_agent_config_storer = SuperAgentConfigStoreFile::new(&cli.get_config_path());

    let opamp_client_builder: Option<OpAMPHttpBuilder> = super_agent_config_storer
        .load()?
        .opamp
        .as_ref()
        .map(|opamp_config| OpAMPHttpBuilder::new(opamp_config.clone()));

    // enable remote config store
    if opamp_client_builder.is_some() {
        super_agent_config_storer = super_agent_config_storer.with_remote()?;
    }

    let instance_id_getter = ULIDInstanceIDGetter::default();

    Ok(run_super_agent(
        super_agent_config_storer,
        ctx,
        opamp_client_builder,
        instance_id_getter,
    )?)
}

fn run_super_agent(
    config_storer: SuperAgentConfigStoreFile,
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_client_builder: Option<OpAMPHttpBuilder>,
    instance_id_getter: ULIDInstanceIDGetter,
) -> Result<(), AgentError> {
    #[cfg(all(unix, feature = "onhost"))]
    if !nix::unistd::Uid::effective().is_root() {
        panic!("Program must run as root");
    }

    #[cfg(feature = "onhost")]
    let sub_agent_builder =
        newrelic_super_agent::sub_agent::on_host::builder::OnHostSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
        );

    // Disabled when --all-features
    #[cfg(all(not(feature = "onhost"), feature = "k8s"))]
    let sub_agent_builder =
        newrelic_super_agent::sub_agent::k8s::builder::K8sSubAgentBuilder::default();

    info!("Starting the super agent");
    SuperAgent::new(
        LocalEffectiveAgentsAssembler::default(),
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        HashRepositoryFile::default(),
        sub_agent_builder,
        config_storer,
    )
    .run(ctx)
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
