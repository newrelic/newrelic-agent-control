use crate::{
    opamp::{hash_repository::HashRepository, remote_config::RemoteConfigError},
    sub_agent::SubAgentBuilder,
    super_agent::{
        config_storer::storer::{
            SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer,
        },
        error::AgentError,
        super_agent::{SuperAgent, SuperAgentCallbacks},
    },
};
use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::StartedClient;

impl<S, O, HR, SL> SuperAgent<S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
{
    pub(crate) fn invalid_remote_config(
        &self,
        remote_config_error: RemoteConfigError,
    ) -> Result<(), AgentError> {
        let Some(opamp_client) = &self.opamp_client else {
            unreachable!();
        };

        let RemoteConfigError::InvalidConfig(hash, error) = remote_config_error else {
            unreachable!();
        };

        let error_message = format!("invalid remote config: {}", error);
        opamp_client.set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: hash.into_bytes(),
            error_message: error,
            status: RemoteConfigStatuses::Failed as i32,
        })?;
        // report unhealthy so the customers can know that the remote config is invalid
        self.report_unhealthy(error_message)?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::remote_config::RemoteConfigError::InvalidConfig;
    use crate::sub_agent::test::MockSubAgentBuilderMock;
    use crate::super_agent::config_storer::storer::tests::MockSubAgentsConfigStore;
    use crate::super_agent::SuperAgent;
    use opamp_client::http::HttpClientError;
    use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
    use opamp_client::ClientError;
    use std::sync::Arc;

    #[test]
    fn test_invalid_remote_config() {
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let sub_agents_config_store = MockSubAgentsConfigStore::new();
        let hash_repository_mock = MockHashRepositoryMock::new();

        let hash = String::from("a-hash");
        let error = String::from("some error");

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: hash.clone().into_bytes(),
            error_message: error.clone(),
            status: RemoteConfigStatuses::Failed as i32,
        });
        started_client.should_set_unhealthy();

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(started_client),
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            sub_agents_config_store,
        );

        let invalid_remote_config = InvalidConfig(hash.clone(), error.clone());
        let res = super_agent.invalid_remote_config(invalid_remote_config);
        assert!(res.is_ok());
    }

    #[test]
    fn test_invalid_remote_config_error_reporting_status() {
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let sub_agents_config_store = MockSubAgentsConfigStore::new();
        let hash_repository_mock = MockHashRepositoryMock::new();

        let hash = String::from("a-hash");
        let error = String::from("some error");

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_not_set_remote_config_status(
            RemoteConfigStatus {
                last_remote_config_hash: hash.clone().into_bytes(),
                error_message: error.clone(),
                status: RemoteConfigStatuses::Failed as i32,
            },
            ClientError::ConnectFailedCallback(String::from("some error message")),
        );

        started_client.should_set_unhealthy();

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(started_client),
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            sub_agents_config_store,
        );

        let invalid_remote_config = InvalidConfig(hash.clone(), error.clone());
        let res = super_agent.invalid_remote_config(invalid_remote_config);
        assert!(res.is_err());

        let err = res.unwrap_err();
        assert_eq!(
            err.to_string(),
            "`Client error. Handling via `on_connect_failed`.`"
        );
    }

    #[test]
    fn test_invalid_remote_config_error_reporting_health() {
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let sub_agents_config_store = MockSubAgentsConfigStore::new();
        let hash_repository_mock = MockHashRepositoryMock::new();

        let hash = String::from("a-hash");
        let error = String::from("some error");

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: hash.clone().into_bytes(),
            error_message: error.clone(),
            status: RemoteConfigStatuses::Failed as i32,
        });

        started_client.should_not_set_health(ClientError::from(HttpClientError::UreqError(
            String::from("some ureq error"),
        )));

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(started_client),
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            sub_agents_config_store,
        );

        let invalid_remote_config = InvalidConfig(hash.clone(), error.clone());
        let res = super_agent.invalid_remote_config(invalid_remote_config);
        assert!(res.is_err());

        let err = res.unwrap_err();
        assert_eq!(err.to_string(), "```some ureq error```");
    }
}
