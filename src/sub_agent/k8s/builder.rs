use opamp_client::operation::callbacks::Callbacks;

use crate::config::super_agent_configs::SubAgentConfig;
use crate::context::Context;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_opamp_and_start_client;
use crate::super_agent::super_agent::SuperAgentEvent;
use crate::{
    config::super_agent_configs::AgentID,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{error::SubAgentBuilderError, logger::Event, SubAgentBuilder},
};
use std::collections::HashMap;

use super::sub_agent::NotStartedSubAgentK8s;

pub struct K8sSubAgentBuilder<'a, C, O, I>
where
    C: Callbacks,
    O: OpAMPClientBuilder<C>,
    I: InstanceIDGetter,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,

    // Needed to include this in the struct to avoid the compiler complaining about not using the type parameter `C`.
    // It's actually used as a generic parameter for the `OpAMPClientBuilder` instance bound by type parameter `O`.
    // Feel free to remove this when the actual implementations (Callbacks instance for K8s agents) make it redundant!
    _callbacks: std::marker::PhantomData<C>,
}

impl<'a, C, O, I> K8sSubAgentBuilder<'a, C, O, I>
where
    C: Callbacks,
    O: OpAMPClientBuilder<C>,
    I: InstanceIDGetter,
{
    pub fn new(opamp_builder: Option<&'a O>, instance_id_getter: &'a I) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,

            _callbacks: std::marker::PhantomData,
        }
    }
}

impl<'a, C, O, I> SubAgentBuilder for K8sSubAgentBuilder<'a, C, O, I>
where
    C: Callbacks,
    O: OpAMPClientBuilder<C>,
    I: InstanceIDGetter,
{
    type NotStartedSubAgent = NotStartedSubAgentK8s<C, O::Client>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        _tx: std::sync::mpsc::Sender<Event>,
        ctx: Context<Option<SuperAgentEvent>>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let maybe_opamp_client = build_opamp_and_start_client(
            ctx,
            self.opamp_builder,
            self.instance_id_getter,
            agent_id.clone(),
            &sub_agent_config.agent_type,
            HashMap::from([]), // TODO: check if we need to set non_identifying_attributes
        )?;

        // TODO: build CRs supervisors and inject them into the NotStartedSubAgentK8s

        Ok(NotStartedSubAgentK8s::new(agent_id, maybe_opamp_client))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::opamp::callbacks::tests::MockCallbacksM;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::operations::start_settings;
    use crate::{
        opamp::client_builder::test::MockOpAMPClientBuilderMock,
        sub_agent::{NotStartedSubAgent, StartedSubAgent},
    };
    use std::{collections::HashMap, sync::mpsc::channel};

    #[test]
    fn build_start_stop() {
        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let mut opamp_builder: MockOpAMPClientBuilderMock<MockCallbacksM> =
            MockOpAMPClientBuilderMock::new();
        let sub_agent_config = sub_agent_config();
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::new(),
        );
        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            |_, _, _| {
                let mut started_client = MockStartedOpAMPClientMock::new();
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
        let ctx: Context<Option<SuperAgentEvent>> = Context::new();
        let started_agent = builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
                tx,
                ctx,
            )
            .unwrap() // Not started agent
            .run()
            .unwrap();
        assert!(started_agent.stop().is_ok())
    }

    fn sub_agent_config() -> SubAgentConfig {
        // TODO: setup k8s runtime_config here. Eg: `final_agent.runtime_config.deployment.k8s = ...`
        SubAgentConfig {
            agent_type: "some_agent".into(),
        }
    }
}
