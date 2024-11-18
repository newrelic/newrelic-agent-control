use crate::agent_type::health_config::{HealthCheckInterval, OnHostHealthConfig};
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::health::health_checker::{spawn_health_checker, HealthCheckerError};
use crate::sub_agent::health::on_host::health_checker::HealthCheckerType;
use crate::sub_agent::health::with_start_time::StartTime;
use crate::super_agent::config::AgentID;
use tracing::error;

pub struct HealthCheckerNotStarted {
    agent_id: AgentID,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    health_checker: HealthCheckerType,
    start_time: StartTime,
    interval: HealthCheckInterval,
}
pub struct HealthCheckerStarted {
    agent_id: AgentID,
    cancel_publisher: EventPublisher<()>,
}
impl HealthCheckerNotStarted {
    pub fn try_new(
        agent_id: AgentID,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        health_config: OnHostHealthConfig,
    ) -> Result<Self, HealthCheckerError> {
        let start_time = StartTime::now();
        let interval = health_config.interval;
        let health_checker = HealthCheckerType::try_new(health_config, start_time)?;

        Ok(HealthCheckerNotStarted {
            agent_id,
            sub_agent_internal_publisher,
            health_checker,
            start_time,
            interval,
        })
    }
    pub fn start(self) -> HealthCheckerStarted {
        let (health_check_cancel_publisher, health_check_cancel_consumer) = pub_sub();

        spawn_health_checker(
            self.agent_id.clone(),
            self.health_checker,
            health_check_cancel_consumer,
            self.sub_agent_internal_publisher,
            self.interval,
            self.start_time,
        );

        HealthCheckerStarted {
            agent_id: self.agent_id,
            cancel_publisher: health_check_cancel_publisher,
        }
    }
}

impl HealthCheckerStarted {
    pub fn stop(self) {
        let _ = self.cancel_publisher.publish(()).inspect_err(|err| {
            error!(
                agent_id = %self.agent_id,
                %err ,
                "could not cancel health checker thread"
            );
        });
    }
}
