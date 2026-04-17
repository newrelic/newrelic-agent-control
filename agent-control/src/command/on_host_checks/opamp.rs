use opamp_client::StartedClient;
use tracing::info;

use crate::{
    agent_control::run::{
        RunningMode,
        on_host::{
            ac_identifiers, build_ac_opamp_start_settings, opamp_client_builder,
            start_ac_opamp_client,
        },
    },
    command::on_host_checks::config::VerifiedConfig,
    opamp::instance_id::{getter::InstanceIDWithIdentifiersGetter, storer::Storer},
    sub_agent::identity::AgentIdentity,
};

pub fn check_connectivity(
    verified_config: VerifiedConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let identifiers = ac_identifiers(&verified_config.agent_control_config)?;

    let instance_id_storer = Storer::from(verified_config.file_store.clone());
    let instance_id_getter =
        InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers.clone());

    let opamp_client_builder = opamp_client_builder(
        verified_config.local_dir.clone(),
        verified_config.maybe_opamp.clone().unwrap(),
        verified_config.proxy_config.clone(),
        verified_config.yaml_config_repository.clone(),
    );

    // TL;DR
    // We leverage code from the normal mode of execution of Agent Control (AC), which makes it
    // simpler and keeps the environments similar at the cost of doing some unnecessary work.
    //
    //
    // The config is built exactly the same in dry-run mode as when we run AC normally.
    // This means the config, identifiers, instance ID, and other relevant information
    // are identical to what we'd have when starting AC in production.
    //
    // Advantages:
    // - Code reuse
    // - Environment parity with normal execution mode
    //
    // Disadvantages:
    // - We must stop the OpAMP client immediately after starting it to prevent spawning a background thread
    // - Even when calling `stop`, the thread might still get spawned
    // - The check sends an `AgentToServer` message and processes a `ServerToAgent` via
    //   `process_message`, doing more work than strictly necessary for connectivity verification
    let agent_identity = AgentIdentity::new_agent_control_identity();
    let settings = build_ac_opamp_start_settings(
        &instance_id_getter,
        &agent_identity,
        &identifiers,
        RunningMode::Verify,
    )?;
    let (client, _consumer) =
        start_ac_opamp_client(&opamp_client_builder, agent_identity, settings)?;
    client.stop()?;

    info!("OpAMP connectivity check successful");

    Ok(())
}
