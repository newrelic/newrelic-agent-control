use newrelic_super_agent::config::store::{SuperAgentConfigStore, SuperAgentConfigStoreFile};
use newrelic_super_agent::opamp::instance_id::getter::ULIDInstanceIDGetter;
use newrelic_super_agent::opamp::instance_id::{self, Identifiers, Storer};
use newrelic_super_agent::opamp::remote_config_hash::HashRepositoryFile;
use newrelic_super_agent::sub_agent::values::values_repository::ValuesRepositoryFile;
use newrelic_super_agent::super_agent::error::AgentError;
use newrelic_super_agent::super_agent::opamp::client_builder::SuperAgentOpAMPHttpBuilder;
use newrelic_super_agent::super_agent::super_agent::{
    super_agent_fqn, SuperAgent, SuperAgentEvent,
};
use newrelic_super_agent::{cli::Cli, context::Context, logging::Logging};
use nix::unistd::gethostname;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::error::Error;
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
    let ctx: Context<Option<SuperAgentEvent>> = Context::new();

    info!("Creating the signal handler");
    create_shutdown_signal_handler(ctx.clone())?;

    let mut super_agent_config_storer = SuperAgentConfigStoreFile::new(&cli.get_config_path());

    let opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder> = super_agent_config_storer
        .load()?
        .opamp
        .as_ref()
        .map(|opamp_config| SuperAgentOpAMPHttpBuilder::new(opamp_config.clone()));

    // enable remote config store
    if opamp_client_builder.is_some() {
        super_agent_config_storer = super_agent_config_storer.with_remote()?;
    }

    #[cfg(all(not(feature = "onhost"), feature = "k8s"))]
    let instance_id_getter = ULIDInstanceIDGetter::try_with_identifiers(
        "newrelic".to_string(),
        instance_id::get_identifiers("fake_cluster".to_string()),
    )
    .await?;
    #[cfg(feature = "onhost")]
    let instance_id_getter = ULIDInstanceIDGetter::try_with_identifiers(Identifiers::default())?;

    #[cfg(any(feature = "onhost", feature = "k8s"))]
    return Ok(run_super_agent(
        super_agent_config_storer,
        ctx,
        opamp_client_builder,
        instance_id_getter,
    )?);

    #[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
    Ok(())
}

#[cfg(feature = "onhost")]
fn run_super_agent(
    config_storer: SuperAgentConfigStoreFile,
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder>,
    instance_id_getter: ULIDInstanceIDGetter<Storer>,
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

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
fn run_super_agent(
    config_storer: SuperAgentConfigStoreFile,
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_client_builder: Option<SuperAgentOpAMPHttpBuilder>,
    instance_id_getter: ULIDInstanceIDGetter<Storer>,
) -> Result<(), AgentError> {
    use newrelic_super_agent::{
        config::super_agent_configs::AgentID, opamp::operations::build_opamp_and_start_client,
    };

    let hash_repository = HashRepositoryFile::default();
    let sub_agent_hash_repository = HashRepositoryFile::new_sub_agent_repository();

    let sub_agent_builder = newrelic_super_agent::sub_agent::k8s::builder::K8sSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
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

fn super_agent_opamp_non_identifying_attributes() -> HashMap<String, DescriptionValueType> {
    HashMap::from([(
        "host.name".to_string(),
        gethostname()
            .unwrap_or_default()
            .into_string()
            .unwrap()
            .into(),
    )])
}
