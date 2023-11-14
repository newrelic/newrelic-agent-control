use crate::{
    config::{agent_type::agent_types::FinalAgent, super_agent_configs::AgentID},
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{error::SubAgentBuilderError, logger::Event, SubAgentBuilder},
    super_agent::instance_id::InstanceIDGetter,
};

use super::sub_agent::NotStartedSubAgentK8s;

pub struct K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
}

impl<'a, O, I> K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    pub fn new(opamp_builder: Option<&'a O>, instance_id_getter: &'a I) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
        }
    }
}

impl<'a, O, I> SubAgentBuilder for K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    type NotStartedSubAgent = NotStartedSubAgentK8s<'a, O, I>;

    fn build(
        &self,
        agent: FinalAgent,
        agent_id: AgentID,
        _tx: std::sync::mpsc::Sender<Event>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        // TODO: build CRs supervisors and inject them into the NotStartedSubAgentK8s
        Ok(NotStartedSubAgentK8s::new(
            agent_id,
            agent.agent_type(),
            self.opamp_builder,
            self.instance_id_getter,
        ))
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, sync::mpsc::channel};

    use opamp_client::{
        capabilities,
        opamp::proto::AgentCapabilities,
        operation::settings::{AgentDescription, StartSettings},
    };

    use crate::{
        opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock},
        sub_agent::{NotStartedSubAgent, StartedSubAgent},
        super_agent::instance_id::test::MockInstanceIDGetterMock,
    };

    use super::*;

    #[test]
    fn build_start_stop() {
        // opamp builder mock
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let start_settings = start_settings();
        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client.should_stop(1);
                Ok(started_client)
            },
        );
        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get("k8s-test".to_string(), "k8s-test-instance-id".to_string());

        let builder = K8sSubAgentBuilder::new(Some(&opamp_builder), &instance_id_getter);

        let (tx, _) = channel();

        let started_agent = builder
            .build(final_agent(), AgentID::new("k8s-test").unwrap(), tx)
            .unwrap() // Not started agent
            .run()
            .unwrap();
        assert!(started_agent.stop().is_ok())
    }

    fn final_agent() -> FinalAgent {
        let mut final_agent = FinalAgent::default();
        // TODO: setup k8s runtime_config here. Eg: `final_agent.runtime_config.deployment.k8s = ...`
        final_agent
    }

    fn start_settings() -> StartSettings {
        StartSettings {
            instance_id: "k8s-test-instance-id".to_string(),
            capabilities: capabilities!(AgentCapabilities::ReportsHealth),
            agent_description: AgentDescription {
                identifying_attributes: HashMap::from([
                    ("service.name".to_string(), "".into()),
                    ("service.namespace".to_string(), "".into()),
                    ("service.version".to_string(), "".into()),
                ]),
                non_identifying_attributes: HashMap::from([]),
            },
        }
    }
}
