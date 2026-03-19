use opamp_client::StartedClient;
use tracing::info;

use crate::{
    agent_control::run::on_host::{ac_identifiers, opamp_client_builder, start_ac_opamp_client},
    command::on_host_checks::config::VerifiedConfig,
    opamp::instance_id::{getter::InstanceIDWithIdentifiersGetter, storer::Storer},
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

    // We are starting and immediately stopping the client just to check connectivity.
    // The client performs a connectivity check as part of its startup process.
    // However, we don't need the client to stay alive after the initial check, so we can stop it right away.
    //
    // This approach avoids having to implement a separate connectivity check logic, and it leverages the existing functionality of the OpAMP client.
    // It comes at a cost. A thread might be spawned between the start and stop, which we don't need. Besides, messages will be processed with
    // `process_message` (as part of the initial check), which is not needed.
    //
    // Long short story, the implementation leverages existing functionality at the cost of doing some unnecessary work.
    let (client, _consumer) =
        start_ac_opamp_client(&opamp_client_builder, &instance_id_getter, &identifiers)?;
    client.stop()?;

    info!("OpAMP connectivity check successful");

    Ok(())
}
