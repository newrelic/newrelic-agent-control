use newrelic_super_agent::cli::Cli;
use newrelic_super_agent::event::channel::{pub_sub, EventConsumer, EventPublisher};
use newrelic_super_agent::event::SuperAgentEvent;
use newrelic_super_agent::opamp::instance_id::getter::ULIDInstanceIDGetter;
use newrelic_super_agent::super_agent::error::AgentError;
use newrelic_super_agent::super_agent::opamp::client_builder::SuperAgentOpAMPHttpBuilder;
use newrelic_super_agent::super_agent::store::{SuperAgentConfigLoader, SuperAgentConfigStoreFile};
use newrelic_super_agent::super_agent::super_agent::{super_agent_fqn, SuperAgent};
use newrelic_super_agent::utils::hostname::HostnameGetter;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
use std::sync::Arc;
use tracing::{error, info};

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::init_super_agent_cli();

    let mut super_agent_config_storer = SuperAgentConfigStoreFile::new(&cli.get_config_path());

    let super_agent_config = super_agent_config_storer.load()?;

    // init logging singleton
    // If file logging is enabled, this will return a `WorkerGuard` value that needs to persist
    // as long as we want the logs to be written to file, hence, we assign it here so it is dropped
    // when the program exits.
    let _guard = super_agent_config.log.try_init()?;

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
    let (super_agent_publisher, super_agent_consumer) = pub_sub();

    info!("Creating the signal handler");
    create_shutdown_signal_handler(super_agent_publisher)?;

    let opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder> = super_agent_config
        .opamp
        .as_ref()
        .map(|opamp_config| SuperAgentOpAMPHttpBuilder::new(opamp_config.clone()));

    // enable remote config store
    if opamp_client_builder.is_some() {
        super_agent_config_storer = super_agent_config_storer.with_remote();
    }

    #[cfg(any(feature = "onhost", feature = "k8s"))]
    return Ok(run_super_agent(
        super_agent_config_storer,
        super_agent_consumer,
        opamp_client_builder,
    )
    .inspect_err(|err| {
        error!(
            "The super agent main process exited with an error: {}",
            err.to_string()
        )
    })?);

    #[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
    Ok(())
}

#[cfg(feature = "onhost")]
fn run_super_agent(
    config_storer: SuperAgentConfigStoreFile,
    super_agent_consumer: EventConsumer<SuperAgentEvent>,
    opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::opamp::hash_repository::HashRepositoryFile;
    use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
    use newrelic_super_agent::opamp::operations::build_opamp_and_start_client;
    use newrelic_super_agent::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
    use newrelic_super_agent::sub_agent::values::values_repository::{
        ValuesRepository, ValuesRepositoryFile,
    };
    use newrelic_super_agent::super_agent::config::AgentID;
    use newrelic_super_agent::{
        sub_agent::on_host::event_processor_builder::EventProcessorBuilder,
        sub_agent::opamp::client_builder::SubAgentOpAMPHttpBuilder,
    };

    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        error!("Program must run as root");
        std::process::exit(1);
    }

    let instance_id_getter = ULIDInstanceIDGetter::default()
        .with_identifiers(IdentifiersProvider::default().provide().unwrap_or_default());

    let hash_repository = HashRepositoryFile::default();
    let agents_assembler = LocalEffectiveAgentsAssembler::default().with_remote();
    // HashRepo and ValuesRepo needs to be shared between threads
    let sub_agent_hash_repository = Arc::new(HashRepositoryFile::new_sub_agent_repository());
    let values_repository = Arc::new(ValuesRepositoryFile::default());
    let sub_agent_event_processor_builder =
        EventProcessorBuilder::new(sub_agent_hash_repository.clone(), values_repository.clone());

    let sub_agent_opamp_builder = opamp_client_builder
        .as_ref()
        .map(SubAgentOpAMPHttpBuilder::from);
    let sub_agent_builder =
        newrelic_super_agent::sub_agent::on_host::builder::OnHostSubAgentBuilder::new(
            sub_agent_opamp_builder.as_ref(),
            &instance_id_getter,
            sub_agent_hash_repository,
            &agents_assembler,
            &sub_agent_event_processor_builder,
        );

    info!("Starting the super agent");

    let (super_agent_opamp_publisher, super_agent_opamp_consumer) = pub_sub();

    let maybe_client = build_opamp_and_start_client(
        super_agent_opamp_publisher.clone(),
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        AgentID::new_super_agent_id(),
        &super_agent_fqn(),
        super_agent_opamp_non_identifying_attributes(),
    )?;

    if maybe_client.is_none() {
        // Delete remote values
        values_repository.delete_remote_all()?;
    }

    SuperAgent::new(
        maybe_client,
        &hash_repository,
        sub_agent_builder,
        Arc::new(config_storer),
    )
    .run(super_agent_consumer, super_agent_opamp_consumer)
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
fn run_super_agent(
    config_storer: SuperAgentConfigStoreFile,
    super_agent_consumer: EventConsumer<SuperAgentEvent>,
    opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::k8s::garbage_collector::NotStartedK8sGarbageCollector;
    use newrelic_super_agent::opamp::hash_repository::HashRepositoryFile;
    use newrelic_super_agent::opamp::instance_id;
    use newrelic_super_agent::opamp::operations::build_opamp_and_start_client;
    use newrelic_super_agent::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
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

    let hash_repository = HashRepositoryFile::default();
    let k8s_config = config_storer.load()?.k8s.ok_or(AgentError::K8sConfig())?;

    let k8s_client = Arc::new(
        newrelic_super_agent::k8s::client::SyncK8sClient::try_new_with_reflectors(
            runtime,
            k8s_config.namespace.clone(),
            k8s_config.cr_type_meta.clone(),
        )
        .map_err(|e| AgentError::ExternalError(e.to_string()))?,
    );

    let instance_id_getter = ULIDInstanceIDGetter::try_with_identifiers(
        k8s_client.clone(),
        instance_id::get_identifiers(k8s_config.cluster_name.clone()),
    )?;

    let agents_assembler = {
        #[cfg(feature = "custom-local-path")]
        {
            use newrelic_super_agent::sub_agent::persister::config_persister_file::ConfigurationPersisterFile;
            use newrelic_super_agent::super_agent::defaults::SUPER_AGENT_DATA_DIR;

            let cli = Cli::init_super_agent_cli();
            let mut values_repo = newrelic_super_agent::sub_agent::values::values_repository::ValuesRepositoryFile::default();
            let mut config_persister = ConfigurationPersisterFile::default();
            let mut temp_assembler = LocalEffectiveAgentsAssembler::default();

            if let Some(base_dir) = cli.get_local_path() {
                if base_dir.is_empty() {
                    return Err(AgentError::InvalidArgumentError(
                        "Base directory cannot be empty".to_string(),
                    ));
                }

                values_repo = values_repo.with_base_dir(base_dir);
                config_persister = ConfigurationPersisterFile::new(std::path::Path::new(&format!(
                    "{}{}",
                    base_dir, SUPER_AGENT_DATA_DIR,
                )));
                temp_assembler = temp_assembler.with_base_dir(base_dir);
            }

            temp_assembler
                .with_values_repository(values_repo)
                .with_config_persister(config_persister)
        }
        #[cfg(not(feature = "custom-local-path"))]
        {
            LocalEffectiveAgentsAssembler::default()
        }
    };

    let sub_agent_builder = newrelic_super_agent::sub_agent::k8s::builder::K8sSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        k8s_client.clone(),
        &agents_assembler,
        k8s_config.clone(),
    );

    info!("Starting the super agent");
    let (opamp_publisher, opamp_consumer) = pub_sub();

    let mut non_identifying_attributes = super_agent_opamp_non_identifying_attributes();
    non_identifying_attributes.insert("cluster.name".to_string(), k8s_config.cluster_name.into());

    let maybe_client = build_opamp_and_start_client(
        opamp_publisher.clone(),
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        AgentID::new_super_agent_id(),
        &super_agent_fqn(),
        non_identifying_attributes,
    )?;

    let config_storer = Arc::new(config_storer);

    let gcc = NotStartedK8sGarbageCollector::new(config_storer.clone(), k8s_client);
    let _started_gcc = gcc.start();

    SuperAgent::new(
        maybe_client,
        &hash_repository,
        sub_agent_builder,
        config_storer,
    )
    .run(super_agent_consumer, opamp_consumer)
}

fn create_shutdown_signal_handler(
    publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
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
