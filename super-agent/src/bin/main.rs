use newrelic_super_agent::cli::Cli;
use newrelic_super_agent::event::channel::{pub_sub, EventConsumer, EventPublisher};
use newrelic_super_agent::event::SuperAgentEvent;
use newrelic_super_agent::opamp::client_builder::OpAMPHttpClientBuilder;
use newrelic_super_agent::opamp::instance_id::getter::ULIDInstanceIDGetter;
use newrelic_super_agent::opamp::instance_id::Identifiers;
use newrelic_super_agent::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use newrelic_super_agent::sub_agent::event_processor_builder::EventProcessorBuilder;
use newrelic_super_agent::super_agent::error::AgentError;
use newrelic_super_agent::super_agent::store::{SuperAgentConfigLoader, SuperAgentConfigStoreFile};
use newrelic_super_agent::super_agent::{super_agent_fqn, SuperAgent};
use newrelic_super_agent::utils::binary_metadata::binary_metadata;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tracing::{debug, error, info};

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::init_super_agent_cli();

    if cli.print_version() {
        println!("{}", binary_metadata());
        return Ok(());
    }

    let mut super_agent_config_storer = SuperAgentConfigStoreFile::new(&cli.get_config_path());

    let super_agent_config = super_agent_config_storer.load().inspect_err(|err| {
        error!(
            "Could not read Super Agent config from {}: {}",
            super_agent_config_storer.config_path().to_string_lossy(),
            err
        )
    })?;

    // init logging singleton
    // If file logging is enabled, this will return a `WorkerGuard` value that needs to persist
    // as long as we want the logs to be written to file, hence, we assign it here so it is dropped
    // when the program exits.
    let _guard = super_agent_config.log.try_init()?;
    info!("Starting NewRelic Super Agent");

    info!("{}", binary_metadata());
    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);

        #[cfg(feature = "onhost")]
        println!("Feature: onhost");
        #[cfg(feature = "k8s")]
        println!("Feature: k8s");

        return Ok(());
    }

    debug!("Creating the global context");
    let (super_agent_publisher, super_agent_consumer) = pub_sub();

    debug!("Creating the signal handler");
    create_shutdown_signal_handler(super_agent_publisher)?;

    let opamp_client_builder: Option<OpAMPHttpClientBuilder> = super_agent_config
        .opamp
        .as_ref()
        .map(|opamp_config| OpAMPHttpClientBuilder::new(opamp_config.clone()));

    // enable remote config store
    if opamp_client_builder.is_some() {
        super_agent_config_storer = super_agent_config_storer.with_remote();
    }

    #[cfg(any(feature = "onhost", feature = "k8s"))]
    run_super_agent(
        Arc::new(super_agent_config_storer),
        super_agent_consumer,
        opamp_client_builder,
    )
    .inspect_err(|err| {
        error!(
            "The super agent main process exited with an error: {}",
            err.to_string()
        )
    })?;

    info!("Exiting gracefully");
    Ok(())
}

#[cfg(feature = "onhost")]
fn run_super_agent(
    config_storer: Arc<SuperAgentConfigStoreFile>,
    super_agent_consumer: EventConsumer<SuperAgentEvent>,
    opamp_client_builder: Option<OpAMPHttpClientBuilder>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::agent_type::renderer::TemplateRenderer;
    use newrelic_super_agent::opamp::hash_repository::HashRepositoryFile;
    use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
    use newrelic_super_agent::opamp::operations::build_opamp_and_start_client;
    use newrelic_super_agent::sub_agent::persister::config_persister_file::ConfigurationPersisterFile;
    use newrelic_super_agent::sub_agent::values::values_repository::ValuesRepository;
    use newrelic_super_agent::sub_agent::values::ValuesRepositoryFile;
    use newrelic_super_agent::super_agent::config::AgentID;

    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        error!("Program must run as root");
        std::process::exit(1);
    }

    let identifiers = IdentifiersProvider::default().provide().unwrap_or_default();
    //Print identifiers for troubleshooting
    print_identifiers(&identifiers);

    let non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);

    let instance_id_getter = ULIDInstanceIDGetter::default().with_identifiers(identifiers);

    let values_repository = Arc::new(ValuesRepositoryFile::default().with_remote());
    let hash_repository = Arc::new(HashRepositoryFile::default());
    let agents_assembler = LocalEffectiveAgentsAssembler::new(values_repository.clone())
        .with_remote()
        .with_renderer(
            TemplateRenderer::default()
                .with_config_persister(ConfigurationPersisterFile::default()),
        );
    let sub_agent_hash_repository = Arc::new(HashRepositoryFile::new_sub_agent_repository());
    let sub_agent_event_processor_builder =
        EventProcessorBuilder::new(sub_agent_hash_repository.clone(), values_repository.clone());

    let sub_agent_builder =
        newrelic_super_agent::sub_agent::on_host::builder::OnHostSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
            sub_agent_hash_repository,
            &agents_assembler,
            &sub_agent_event_processor_builder,
        );

    let (super_agent_opamp_publisher, super_agent_opamp_consumer) = pub_sub();

    let maybe_client = build_opamp_and_start_client(
        super_agent_opamp_publisher.clone(),
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        AgentID::new_super_agent_id(),
        &super_agent_fqn(),
        non_identifying_attributes,
    )?;

    if maybe_client.is_none() {
        // Delete remote values
        info!("No OpAMP settings configured. Cleaning remote configs");
        values_repository.delete_remote_all()?;
    } else {
        info!("Super Agent OpAMP client started");
    }

    SuperAgent::new(
        maybe_client,
        hash_repository,
        sub_agent_builder,
        config_storer,
    )
    .run(super_agent_consumer, super_agent_opamp_consumer)
}

fn print_identifiers(identifiers: &Identifiers) {
    info!("Instance Identifiers: {}", identifiers);
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
fn run_super_agent(
    config_storer: Arc<SuperAgentConfigStoreFile>,
    super_agent_consumer: EventConsumer<SuperAgentEvent>,
    opamp_client_builder: Option<OpAMPHttpClientBuilder>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::k8s::garbage_collector::NotStartedK8sGarbageCollector;
    use newrelic_super_agent::k8s::store::K8sStore;
    use newrelic_super_agent::opamp::hash_repository::HashRepositoryConfigMap;
    use newrelic_super_agent::opamp::instance_id;
    use newrelic_super_agent::opamp::operations::build_opamp_and_start_client;
    use newrelic_super_agent::sub_agent::values::ValuesRepositoryConfigMap;
    use newrelic_super_agent::super_agent::config::AgentID;
    use std::sync::OnceLock;

    /// Returns a static reference to a tokio runtime initialized on first usage.
    /// It uses the default tokio configuration (the same that #[tokio::main]).
    // TODO: avoid the need of this global reference
    static RUNTIME_ONCE: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let runtime = RUNTIME_ONCE.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    });

    info!("Starting the k8s client");
    let k8s_config = config_storer.load()?.k8s.ok_or(AgentError::K8sConfig())?;
    let k8s_client = Arc::new(
        newrelic_super_agent::k8s::client::SyncK8sClient::try_new_with_reflectors(
            runtime,
            k8s_config.namespace.clone(),
            k8s_config.cr_type_meta.clone(),
        )
        .map_err(|e| AgentError::ExternalError(e.to_string()))?,
    );

    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let identifiers = instance_id::get_identifiers(k8s_config.cluster_name.clone());
    //Print identifiers for troubleshooting
    print_identifiers(&identifiers);

    let mut non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);
    non_identifying_attributes.insert(
        "cluster.name".to_string(),
        k8s_config.cluster_name.clone().into(),
    );

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(k8s_store.clone(), identifiers)?;

    let values_repository =
        Arc::new(ValuesRepositoryConfigMap::new(k8s_store.clone()).with_remote());
    let agents_assembler =
        LocalEffectiveAgentsAssembler::new(values_repository.clone()).with_remote();
    let hash_repository = Arc::new(HashRepositoryConfigMap::new(k8s_store.clone()));
    let sub_agent_event_processor_builder =
        EventProcessorBuilder::new(hash_repository.clone(), values_repository.clone());

    info!("Creating the k8s sub_agent builder");
    let sub_agent_builder = newrelic_super_agent::sub_agent::k8s::builder::K8sSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        k8s_client.clone(),
        hash_repository.clone(),
        &agents_assembler,
        &sub_agent_event_processor_builder,
        k8s_config.clone(),
    );

    let (opamp_publisher, opamp_consumer) = pub_sub();

    let maybe_client = build_opamp_and_start_client(
        opamp_publisher.clone(),
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        AgentID::new_super_agent_id(),
        &super_agent_fqn(),
        non_identifying_attributes,
    )?;

    let gcc = NotStartedK8sGarbageCollector::new(config_storer.clone(), k8s_client);
    let _started_gcc = gcc.start();

    SuperAgent::new(
        maybe_client,
        hash_repository,
        sub_agent_builder,
        config_storer,
    )
    .run(super_agent_consumer, opamp_consumer)
}

fn create_shutdown_signal_handler(
    publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        info!("Received SIGINT (Ctrl-C). Stopping super agent");
        let _ = publisher
            .publish(SuperAgentEvent::StopRequested)
            .map_err(|_| error!("Could not send super agent stop request"));
    })
    .map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    Ok(())
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
fn super_agent_opamp_non_identifying_attributes(
    _identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    use newrelic_super_agent::utils::hostname::HostnameGetter;

    let hostname = HostnameGetter::default()
        .get()
        .unwrap_or_else(|e| {
            error!("cannot retrieve hostname: {}", e.to_string());
            std::ffi::OsString::from("unknown_hostname")
        })
        .to_string_lossy()
        .to_string();

    HashMap::from([(
        opentelemetry_semantic_conventions::resource::HOST_NAME.to_string(),
        DescriptionValueType::String(hostname),
    )])
}

#[cfg(all(not(feature = "k8s"), feature = "onhost"))]
fn super_agent_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    HashMap::from([
        (
            opentelemetry_semantic_conventions::resource::HOST_NAME.to_string(),
            DescriptionValueType::String(identifiers.hostname.clone()),
        ),
        (
            opentelemetry_semantic_conventions::resource::HOST_ID.to_string(),
            DescriptionValueType::String(identifiers.host_id.clone()),
        ),
    ])
}
