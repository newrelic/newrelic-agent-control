use newrelic_super_agent::cli::Cli;
use newrelic_super_agent::event::channel::{pub_sub, EventConsumer, EventPublisher};
use newrelic_super_agent::event::{ApplicationEvent, SuperAgentEvent};
use newrelic_super_agent::opamp::client_builder::DefaultOpAMPClientBuilder;
use newrelic_super_agent::opamp::http::builder::DefaultHttpClientBuilder;
use newrelic_super_agent::opamp::http::builder::HttpClientBuilder;
use newrelic_super_agent::opamp::instance_id::getter::ULIDInstanceIDGetter;
use newrelic_super_agent::opamp::instance_id::Identifiers;
use newrelic_super_agent::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use newrelic_super_agent::sub_agent::event_processor_builder::EventProcessorBuilder;
use newrelic_super_agent::super_agent::config_storer::storer::SuperAgentConfigLoader;
use newrelic_super_agent::super_agent::config_storer::SuperAgentConfigStoreFile;
use newrelic_super_agent::super_agent::defaults::{
    FLEET_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
};
use newrelic_super_agent::super_agent::error::AgentError;
use newrelic_super_agent::super_agent::http_server::runner::Runner;
use newrelic_super_agent::super_agent::{super_agent_fqn, SuperAgent};
use newrelic_super_agent::utils::binary_metadata::binary_metadata;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::runtime::Runtime;
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

    let sa_local_config_storer = SuperAgentConfigStoreFile::new(&cli.get_config_path());

    let super_agent_config = sa_local_config_storer.load().inspect_err(|err| {
        error!(
            "Could not read Super Agent config from {}: {}",
            sa_local_config_storer.config_path().to_string_lossy(),
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
    let (application_event_publisher, application_event_consumer) = pub_sub();

    debug!("Creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher)?;

    let opamp_client_builder: Option<DefaultOpAMPClientBuilder<_>> =
        super_agent_config.opamp.as_ref().map(|opamp_config| {
            let http_builder = DefaultHttpClientBuilder::new(opamp_config.clone());
            DefaultOpAMPClientBuilder::new(opamp_config.clone(), http_builder)
        });

    // create Super Agent events channel
    let (super_agent_publisher, super_agent_consumer) = pub_sub::<SuperAgentEvent>();

    // Create the Tokio runtime
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );

    // Start the status server if enabled. Stopping control is done through events
    // Dropping _started_runner will force it to wait until the server is gracefully
    // stopped
    let _started_http_server_runner = Runner::start(
        super_agent_config.server.clone(),
        runtime.clone(),
        super_agent_consumer,
        super_agent_config.opamp.clone(),
    );

    #[cfg(any(feature = "onhost", feature = "k8s"))]
    run_super_agent(
        runtime.clone(),
        sa_local_config_storer,
        application_event_consumer,
        opamp_client_builder,
        super_agent_publisher,
    )
    .inspect_err(|err| {
        error!(
            "The super agent main process exited with an error: {}",
            err.to_string()
        )
    })?;

    info!("exiting gracefully");
    Ok(())
}

#[cfg(feature = "onhost")]
fn run_super_agent<C: HttpClientBuilder>(
    _runtime: Arc<Runtime>,
    sa_config_storer: SuperAgentConfigStoreFile,
    application_events_consumer: EventConsumer<ApplicationEvent>,
    opamp_client_builder: Option<DefaultOpAMPClientBuilder<C>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::agent_type::renderer::TemplateRenderer;
    use newrelic_super_agent::opamp::hash_repository::HashRepositoryFile;
    use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
    use newrelic_super_agent::opamp::operations::build_opamp_with_channel;
    use newrelic_super_agent::sub_agent::on_host::builder::OnHostSubAgentBuilder;
    use newrelic_super_agent::sub_agent::persister::config_persister_file::ConfigurationPersisterFile;
    use newrelic_super_agent::sub_agent::values::ValuesRepositoryFile;
    use newrelic_super_agent::super_agent::config::AgentID;

    // enable remote config store
    let config_storer = if opamp_client_builder.is_some() {
        Arc::new(sa_config_storer.with_remote())
    } else {
        Arc::new(sa_config_storer)
    };

    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        error!("Program must run as root");
        std::process::exit(1);
    }

    let config = config_storer.load()?;

    let identifiers_provider = IdentifiersProvider::default()
        .with_host_id(config.host_id)
        .with_fleet_id(config.fleet_id);
    let identifiers = identifiers_provider.provide().unwrap_or_default();
    info!("Instance Identifiers: {}", identifiers);

    let non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);

    let instance_id_getter = ULIDInstanceIDGetter::default().with_identifiers(identifiers);

    let mut vr = ValuesRepositoryFile::default();
    if opamp_client_builder.is_some() {
        vr = vr.with_remote();
    }
    let values_repository = Arc::new(vr);

    let hash_repository = Arc::new(HashRepositoryFile::default());
    let agents_assembler = LocalEffectiveAgentsAssembler::new(values_repository.clone())
        .with_renderer(
            TemplateRenderer::default()
                .with_config_persister(ConfigurationPersisterFile::default()),
        );
    let sub_agent_hash_repository = Arc::new(HashRepositoryFile::new_sub_agent_repository());
    let sub_agent_event_processor_builder =
        EventProcessorBuilder::new(sub_agent_hash_repository.clone(), values_repository.clone());

    let sub_agent_builder = OnHostSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        sub_agent_hash_repository,
        &agents_assembler,
        &sub_agent_event_processor_builder,
        identifiers_provider,
    );

    let (maybe_client, maybe_sa_opamp_consumer) = opamp_client_builder
        .as_ref()
        .map(|builder| {
            build_opamp_with_channel(
                builder,
                &instance_id_getter,
                AgentID::new_super_agent_id(),
                &super_agent_fqn(),
                non_identifying_attributes,
            )
        })
        // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
        .transpose()?
        .map(|(client, consumer)| (Some(client), Some(consumer)))
        .unwrap_or_default();

    SuperAgent::new(
        maybe_client,
        hash_repository,
        sub_agent_builder,
        config_storer,
        super_agent_publisher,
    )
    .run(application_events_consumer, maybe_sa_opamp_consumer)
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
fn run_super_agent<C: HttpClientBuilder>(
    runtime: Arc<Runtime>,
    sa_local_config_storer: SuperAgentConfigStoreFile,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    opamp_client_builder: Option<DefaultOpAMPClientBuilder<C>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::k8s::garbage_collector::NotStartedK8sGarbageCollector;
    use newrelic_super_agent::k8s::store::K8sStore;
    use newrelic_super_agent::opamp::hash_repository::HashRepositoryConfigMap;
    use newrelic_super_agent::opamp::instance_id;
    use newrelic_super_agent::opamp::operations::build_opamp_with_channel;
    use newrelic_super_agent::sub_agent::values::ValuesRepositoryConfigMap;
    use newrelic_super_agent::super_agent::config::AgentID;
    use newrelic_super_agent::super_agent::config_storer::SubAgentsConfigStoreConfigMap;

    info!("Starting the k8s client");
    let config = sa_local_config_storer.load()?;
    let k8s_config = config.k8s.ok_or(AgentError::K8sConfig())?;
    let k8s_client = Arc::new(
        newrelic_super_agent::k8s::client::SyncK8sClient::try_new_with_reflectors(
            runtime,
            k8s_config.namespace.clone(),
            k8s_config.cr_type_meta.clone(),
        )
        .map_err(|e| AgentError::ExternalError(e.to_string()))?,
    );

    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let identifiers =
        instance_id::get_identifiers(k8s_config.cluster_name.clone(), config.fleet_id);
    info!("Instance Identifiers: {}", identifiers);

    let mut non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);
    non_identifying_attributes.insert(
        "cluster.name".to_string(),
        k8s_config.cluster_name.clone().into(),
    );

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(k8s_store.clone(), identifiers)?;

    let mut vr = ValuesRepositoryConfigMap::new(k8s_store.clone());
    if opamp_client_builder.is_some() {
        vr = vr.with_remote();
    }
    let values_repository = Arc::new(vr);

    let agents_assembler = LocalEffectiveAgentsAssembler::new(values_repository.clone());
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

    let (maybe_client, opamp_consumer) = opamp_client_builder
        .as_ref()
        .map(|builder| {
            build_opamp_with_channel(
                builder,
                &instance_id_getter,
                AgentID::new_super_agent_id(),
                &super_agent_fqn(),
                non_identifying_attributes,
            )
        })
        // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
        .transpose()?
        .map(|(client, consumer)| (Some(client), Some(consumer)))
        .unwrap_or_default();

    let sub_agents_config_storer =
        SubAgentsConfigStoreConfigMap::new(k8s_store.clone(), config.dynamic);
    // enable remote config store
    let config_storer = if opamp_client_builder.is_some() {
        Arc::new(sub_agents_config_storer.with_remote())
    } else {
        Arc::new(sub_agents_config_storer)
    };

    let gcc = NotStartedK8sGarbageCollector::new(config_storer.clone(), k8s_client);
    let _started_gcc = gcc.start();

    SuperAgent::new(
        maybe_client,
        hash_repository,
        sub_agent_builder,
        config_storer,
        super_agent_publisher,
    )
    .run(application_event_consumer, opamp_consumer)
}

fn create_shutdown_signal_handler(
    publisher: EventPublisher<ApplicationEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        info!("Received SIGINT (Ctrl-C). Stopping super agent");
        let _ = publisher
            .publish(ApplicationEvent::StopRequested)
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
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    use resource_detection::system::hostname::HostnameGetter;
    let hostname = HostnameGetter::default()
        .get()
        .unwrap_or_else(|e| {
            error!("cannot retrieve hostname: {}", e.to_string());
            std::ffi::OsString::from("unknown_hostname")
        })
        .to_string_lossy()
        .to_string();

    HashMap::from([
        (
            HOST_NAME_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(hostname),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.fleet_id.clone()),
        ),
    ])
}

#[cfg(feature = "onhost")]
fn super_agent_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    use newrelic_super_agent::super_agent::defaults::HOST_ID_ATTRIBUTE_KEY;

    HashMap::from([
        (
            HOST_NAME_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.hostname.clone()),
        ),
        (
            HOST_ID_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.host_id.clone()),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.fleet_id.clone()),
        ),
    ])
}
