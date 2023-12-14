use crossbeam::channel::Receiver;
use crossbeam::channel::{unbounded, Sender};
use newrelic_super_agent::config::store::{SuperAgentConfigStore, SuperAgentConfigStoreFile};
use newrelic_super_agent::event::channel::{channel, EventConsumer, EventPublisher};
use newrelic_super_agent::event::event::{Event, SuperAgentEvent};
use newrelic_super_agent::event::Publisher;
#[cfg(feature = "k8s")]
use newrelic_super_agent::opamp::instance_id;
use newrelic_super_agent::opamp::instance_id::getter::ULIDInstanceIDGetter;
#[cfg(feature = "onhost")]
use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
use newrelic_super_agent::opamp::remote_config_hash::HashRepositoryFile;
use newrelic_super_agent::sub_agent::values::values_repository::ValuesRepositoryFile;
use newrelic_super_agent::super_agent::error::AgentError;
use newrelic_super_agent::super_agent::opamp::client_builder::SuperAgentOpAMPHttpBuilder;
use newrelic_super_agent::super_agent::super_agent::{super_agent_fqn, SuperAgent};
use newrelic_super_agent::utils::hostname::HostnameGetter;
use newrelic_super_agent::{cli::Cli, context::Context, logging::Logging};
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
use tracing::{error, info};

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // init logging singleton
    Logging::try_init()?;

    let cli = Cli::init_super_agent_cli();

    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);

        #[cfg(feature = "onhost")]
        println!("Feature: onhost");
        #[cfg(feature = "k8s")]
        println!("Feature: k8s");

        return Ok(());
    }

    info!("Creating the global context");
    let (cancel_sender, cancel_receiver) = channel();

    info!("Creating the signal handler");
    create_shutdown_signal_handler(cancel_sender)?;

    let mut super_agent_config_storer = SuperAgentConfigStoreFile::new(&cli.get_config_path());

    let super_agent_config = super_agent_config_storer.load()?;

    let opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder> = super_agent_config
        .opamp
        .as_ref()
        .map(|opamp_config| SuperAgentOpAMPHttpBuilder::new(opamp_config.clone()));

    // enable remote config store
    if opamp_client_builder.is_some() {
        super_agent_config_storer = super_agent_config_storer.with_remote()?;
    }

    #[cfg(any(feature = "onhost", feature = "k8s"))]
    return Ok(run_super_agent(
        super_agent_config_storer,
        cancel_receiver,
        opamp_client_builder,
    )?);

    #[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
    Ok(())
}

#[cfg(feature = "onhost")]
fn run_super_agent(
    config_storer: SuperAgentConfigStoreFile,
    cancel_receiver: EventConsumer<SuperAgentEvent>,
    opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::{
        config::super_agent_configs::AgentID, opamp::operations::build_opamp_and_start_client,
        sub_agent::opamp::client_builder::SubAgentOpAMPHttpBuilder,
        super_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler,
    };

    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        panic!("Program must run as root");
    }

    let instance_id_getter =
        ULIDInstanceIDGetter::default().with_identifiers(IdentifiersProvider::default().provide());

    let hash_repository = HashRepositoryFile::default();
    let sub_agent_hash_repository = HashRepositoryFile::new_sub_agent_repository();
    let agents_assembler = LocalEffectiveAgentsAssembler::default().with_remote();

    let sub_agent_opamp_builder = opamp_client_builder
        .as_ref()
        .map(SubAgentOpAMPHttpBuilder::from);
    let sub_agent_builder =
        newrelic_super_agent::sub_agent::on_host::builder::OnHostSubAgentBuilder::new(
            sub_agent_opamp_builder.as_ref(),
            &instance_id_getter,
            &sub_agent_hash_repository,
            &agents_assembler,
        );

    info!("Starting the super agent");
    let values_repository = ValuesRepositoryFile::default();

    let ctx = Context::new();
    let (opamp_sender, opamp_receiver) = channel();

    let maybe_client = build_opamp_and_start_client(
        ctx,
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        AgentID::new_super_agent_id(),
        &super_agent_fqn(),
        super_agent_opamp_non_identifying_attributes(),
    )?;

    SuperAgent::new(
        maybe_client,
        &hash_repository,
        sub_agent_builder,
        config_storer,
        &sub_agent_hash_repository,
        values_repository,
    )
    .run(cancel_receiver, opamp_receiver)
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
fn run_super_agent(
    config_storer: SuperAgentConfigStoreFile,
    ctx: Context<Option<Event>>,
    opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::{
        config::super_agent_configs::AgentID, opamp::operations::build_opamp_and_start_client,
    };

    let hash_repository = HashRepositoryFile::default();
    let sub_agent_hash_repository = HashRepositoryFile::new_sub_agent_repository();
    let k8s_config = config_storer.load()?.k8s.ok_or(AgentError::K8sConfig())?;

    let instance_id_getter =
        futures::executor::block_on(ULIDInstanceIDGetter::try_with_identifiers(
            k8s_config.namespace,
            instance_id::get_identifiers(k8s_config.cluster_name),
        ))?;

    // Initialize K8sExecutor
    // TODO: once we know how we're going to use the K8sExecutor, we might need to refactor and move this.
    let namespace = "default".to_string(); // change to your desired namespace
    let executor = futures::executor::block_on(
        newrelic_super_agent::k8s::executor::K8sExecutor::try_new_with_reflectors(
            namespace,
            k8s_config.cr_type_meta,
        ),
    )
    .map_err(|e| AgentError::ExternalError(e.to_string()))?;
    /////////////////////////

    let sub_agent_builder = newrelic_super_agent::sub_agent::k8s::builder::K8sSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        std::sync::Arc::new(executor),
    );

    info!("Starting the super agent");
    let values_repository = ValuesRepositoryFile::default();
    let maybe_client = build_opamp_and_start_client(
        ctx.clone(),
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        AgentID::new_super_agent_id(),
        &super_agent_fqn(),
        super_agent_opamp_non_identifying_attributes(),
    )?;

    SuperAgent::new(
        maybe_client,
        &hash_repository,
        sub_agent_builder,
        config_storer,
        &sub_agent_hash_repository,
        values_repository,
    )
    .run(ctx)
}

fn create_shutdown_signal_handler(
    ctx: EventPublisher<SuperAgentEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || ctx.publish(SuperAgentEvent::StopRequested)).map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    Ok(())
}

fn super_agent_opamp_non_identifying_attributes() -> HashMap<String, DescriptionValueType> {
    let hostname = HostnameGetter::default()
        .get()
        .unwrap_or_else(|e| {
            error!("cannot retrieve hostname: {}", e.to_string());
            OsString::from("unknown_hostname")
        })
        .to_string_lossy()
        .to_string();

    HashMap::from([(
        "host.name".to_string(),
        DescriptionValueType::String(hostname),
    )])
}
