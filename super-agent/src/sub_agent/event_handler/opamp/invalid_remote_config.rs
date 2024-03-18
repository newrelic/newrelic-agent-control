use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::StartedClient;

use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config::RemoteConfigError;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::SubAgentCallbacks;

impl<C, H, R> EventProcessor<C, H, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    pub(crate) fn invalid_remote_config(
        &self,
        remote_config_error: RemoteConfigError,
    ) -> Result<(), SubAgentError> {
        if let Some(client) = self.maybe_opamp_client.as_ref() {
            if let RemoteConfigError::InvalidConfig(hash, error) = remote_config_error {
                client.set_remote_config_status(RemoteConfigStatus {
                    last_remote_config_hash: hash.into_bytes(),
                    error_message: error,
                    status: RemoteConfigStatuses::Failed as i32,
                })?;
                Ok(())
            } else {
                unreachable!()
            }
        } else {
            unreachable!("got remote config without OpAMP being enabled")
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::remote_config::RemoteConfigError::InvalidConfig;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::event_processor::EventProcessor;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::sub_agent::SubAgentCallbacks;
    use crate::super_agent::config::AgentID;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use std::sync::Arc;

    #[test]
    fn test_error_is_reported_to_opamp() {
        let mut opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        let hash = Hash::new(String::from("some-hash"));

        // report failed config
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: "some error".to_string(),
        };
        opamp_client.should_set_remote_config_status(status);

        let remote_config_error =
            InvalidConfig(String::from("some-hash"), String::from("some error"));

        let event_processor = EventProcessor::new(
            AgentID::new("some-agent-id").unwrap(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        event_processor
            .invalid_remote_config(remote_config_error)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_no_opamp_should_panic() {
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        let remote_config_error =
            InvalidConfig(String::from("some-hash"), String::from("some error"));

        let event_processor = EventProcessor::new(
            AgentID::new("some-agent-id").unwrap(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            None::<MockStartedOpAMPClientMock<SubAgentCallbacks>>,
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        let _ = event_processor.invalid_remote_config(remote_config_error);
    }
}
