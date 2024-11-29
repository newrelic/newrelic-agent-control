use crate::opamp::remote_config::RemoteConfigError;
use crate::sub_agent::error::SubAgentError;
use crate::values::yaml_config::YAMLConfig;
use crate::{
    opamp::{hash_repository::HashRepository, remote_config::RemoteConfig},
    values::yaml_config_repository::YAMLConfigRepository,
};

pub fn store_remote_config_hash_and_values<HS, Y>(
    remote_config: &mut RemoteConfig,
    sub_agent_remote_config_hash_repository: &HS,
    remote_values_repo: &Y,
) -> Result<(), SubAgentError>
where
    HS: HashRepository,
    Y: YAMLConfigRepository,
{
    // Save the configuration hash
    sub_agent_remote_config_hash_repository.save(&remote_config.agent_id, &remote_config.hash)?;
    // The remote configuration can be invalid (checked while deserializing)
    if let Some(err) = remote_config.hash.error_message() {
        return Err(RemoteConfigError::InvalidConfig(remote_config.hash.get(), err).into());
    }
    // Save the configuration values
    match process_remote_config(remote_config) {
        Err(err) => {
            // Store the hash failure if values cannot be obtained from remote config
            remote_config.hash.fail(err.to_string());
            sub_agent_remote_config_hash_repository
                .save(&remote_config.agent_id, &remote_config.hash)?;
            Err(err)
        }
        // Remove previously persisted values when the configuration is empty
        Ok(None) => Ok(remote_values_repo.delete_remote(&remote_config.agent_id)?),
        Ok(Some(agent_values)) => {
            Ok(remote_values_repo.store_remote(&remote_config.agent_id, &agent_values)?)
        }
    }
}

fn process_remote_config(
    remote_config: &RemoteConfig,
) -> Result<Option<YAMLConfig>, SubAgentError> {
    let remote_config_value = remote_config.get_unique()?;

    if remote_config_value.is_empty() {
        return Ok(None);
    }

    Ok(Some(YAMLConfig::try_from(remote_config_value.to_string())?))
}
