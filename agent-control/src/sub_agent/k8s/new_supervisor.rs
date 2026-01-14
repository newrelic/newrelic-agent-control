use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::{
    event::{SubAgentInternalEvent, channel::EventPublisher},
    sub_agent::{
        effective_agents_assembler::EffectiveAgent,
        k8s::supervisor::{NotStartedSupervisorK8s, StartedSupervisorK8s},
        supervisor::{Supervisor, SupervisorStarter, starter::SupervisorStarterError},
    },
    utils::thread_context::ThreadContextStopperError,
};

impl SupervisorStarter for NotStartedSupervisorK8s {
    type Supervisor = StartedSupervisorK8s;
    type Error = SupervisorStarterError;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::Supervisor, Self::Error> {
        info!("Starting k8s supervisor");
        let resources = Arc::new(self.build_dynamic_objects()?);

        let thread_contexts = [
            Some(self.start_k8s_objects_supervisor(resources.clone())),
            self.start_health_check(sub_agent_internal_publisher.clone(), resources.clone())?,
            self.start_version_checker(sub_agent_internal_publisher.clone(), resources.clone()),
            self.start_guid_checker(sub_agent_internal_publisher.clone(), resources),
        ]
        .into_iter()
        .flatten()
        .collect();
        info!("K8s supervisor started");

        // Reuse structures
        let Self {
            agent_identity,
            k8s_client,
            ..
        } = self;

        Ok(StartedSupervisorK8s {
            thread_contexts,
            k8s_client,
            sub_agent_internal_publisher,
            agent_identity,
        })
    }
}

impl Supervisor for StartedSupervisorK8s {
    type ApplyError = SupervisorStarterError;
    type StopError = ThreadContextStopperError;

    fn apply(self, effective_agent: EffectiveAgent) -> Result<Self, Self::ApplyError> {
        // Re-use started supervisor's contents
        let agent_identity = self.agent_identity.clone();
        let k8s_client = self.k8s_client.clone();
        let sub_agent_internal_publisher = self.sub_agent_internal_publisher.clone();
        let k8s_config = effective_agent
            .get_k8s_config()
            .map_err(|e| SupervisorStarterError::ConfigError(e.to_string()))?
            .clone();

        debug!(
            agent_id = %self.agent_identity.id,
            "Applying new configuration to K8s supervisor"
        );

        if let Err(e) = self.stop_threads() {
            warn!(agent_id = %agent_identity.id, "Error stopping threads: {e}");
        }

        // Helper to build dynamic objects from the new config
        let temp_starter =
            NotStartedSupervisorK8s::new(agent_identity, k8s_client.clone(), k8s_config);
        let resources = temp_starter.build_dynamic_objects()?;

        // Apply resources directly
        Self::apply_resources(resources.iter(), k8s_client)?;

        SupervisorStarter::start(temp_starter, sub_agent_internal_publisher)
    }

    fn stop(self) -> Result<(), Self::StopError> {
        self.stop_threads()
    }
}
