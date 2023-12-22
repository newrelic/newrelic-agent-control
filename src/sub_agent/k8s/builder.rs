use super::sub_agent::NotStartedSubAgentK8s;
use crate::config::super_agent_configs::SubAgentConfig;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_opamp_and_start_client;
use crate::{
    config::super_agent_configs::AgentID,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::k8s::supervisor::CRSupervisor,
    sub_agent::{error::SubAgentBuilderError, logger::AgentLog, SubAgentBuilder},
};
use opamp_client::operation::callbacks::Callbacks;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::executor::K8sExecutor;

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
    // client: Client, Should we inject the client?
    executor: Arc<K8sExecutor>,
}

impl<'a, C, O, I> K8sSubAgentBuilder<'a, C, O, I>
where
    C: Callbacks,
    O: OpAMPClientBuilder<C>,
    I: InstanceIDGetter,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        executor: Arc<K8sExecutor>,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,

            _callbacks: std::marker::PhantomData,
            executor,
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
        _tx: std::sync::mpsc::Sender<AgentLog>,
        _sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let (sub_agent_opamp_publisher, _sub_agent_opamp_consumer) = pub_sub();

        let maybe_opamp_client = build_opamp_and_start_client(
            sub_agent_opamp_publisher,
            self.opamp_builder,
            self.instance_id_getter,
            agent_id.clone(),
            &sub_agent_config.agent_type,
            HashMap::from([]), // TODO: check if we need to set non_identifying_attributes
        )?;

        // Clone the executor on each build.
        let supervisor = CRSupervisor::new(Arc::clone(&self.executor.clone()));

        Ok(NotStartedSubAgentK8s::new(
            agent_id,
            maybe_opamp_client,
            supervisor,
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::event::channel::pub_sub;
    use crate::opamp::callbacks::tests::MockCallbacksMock;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::operations::start_settings;
    use crate::{
        k8s::executor::MockK8sExecutor,
        opamp::client_builder::test::MockOpAMPClientBuilderMock,
        sub_agent::{NotStartedSubAgent, StartedSubAgent},
    };
    use mockall::predicate;
    use std::{collections::HashMap, sync::mpsc::channel};

    #[test]
    fn build_start_stop() {
        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let mut opamp_builder: MockOpAMPClientBuilderMock<MockCallbacksMock> =
            MockOpAMPClientBuilderMock::new();
        let sub_agent_config = sub_agent_config();
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::new(),
        );

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        started_client.should_stop(1);

        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            started_client,
        );
        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            &AgentID::new("k8s-test").unwrap(),
            "k8s-test-instance-id".to_string(),
        );

        // instance K8s executor mock
        let mut mock_executor = MockK8sExecutor::default();

        // Set mock executor expectations
        mock_executor
            .expect_apply_dynamic_object()
            //TODO as soon as we are supporting passing which agent to execute we should check these predicates
            .with(predicate::always())
            .times(2)
            .returning(move |_| Ok(()));

        mock_executor
            .expect_has_dynamic_object_changed()
            .times(2)
            .returning(|_| Ok(true));

        mock_executor
            .expect_delete_dynamic_object()
            .with(predicate::always(), predicate::always())
            .times(0) // Expect it to be called 0 times, since it is the garbage collector cleaning it.
            .returning(|_, _| Ok(()));

        let executor = Arc::new(mock_executor);
        let builder = K8sSubAgentBuilder::new(Some(&opamp_builder), &instance_id_getter, executor);

        let (tx, _) = channel();
        let (super_agent_publisher, _super_agent_consumer) = pub_sub();
        let started_agent = builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
                tx,
                super_agent_publisher,
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
