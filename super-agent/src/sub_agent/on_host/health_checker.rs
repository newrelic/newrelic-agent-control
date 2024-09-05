use tracing::error;

use crate::agent_type::health_config::{HealthCheckInterval, OnHostHealthConfig};
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::health::health_checker::{spawn_health_checker, HealthCheckerError};
use crate::sub_agent::health::on_host::health_checker::HealthCheckerType;
use crate::sub_agent::health::with_start_time::StartTime;
use crate::super_agent::config::AgentID;

pub struct HealthChecker<S> {
    agent_id: AgentID,
    state: S,
}
pub struct HealthCheckerNotStarted {
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    health_checker: HealthCheckerType,
    start_time: StartTime,
    interval: HealthCheckInterval,
}
pub struct HealthCheckerStarted {
    cancel_publisher: EventPublisher<()>,
}
impl HealthChecker<HealthCheckerNotStarted> {
    pub fn try_new(
        agent_id: AgentID,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        health_config: OnHostHealthConfig,
    ) -> Result<Self, HealthCheckerError> {
        let start_time = StartTime::now();
        let interval = health_config.interval;
        let health_checker = HealthCheckerType::try_new(health_config, start_time)?;

        Ok(HealthChecker {
            agent_id,
            state: HealthCheckerNotStarted {
                sub_agent_internal_publisher,
                health_checker,
                start_time,
                interval,
            },
        })
    }
    pub fn start(self) -> HealthChecker<HealthCheckerStarted> {
        let (health_check_cancel_publisher, health_check_cancel_consumer) = pub_sub();

        spawn_health_checker(
            self.agent_id.clone(),
            self.state.health_checker,
            health_check_cancel_consumer,
            self.state.sub_agent_internal_publisher,
            self.state.interval,
            self.state.start_time,
        );

        HealthChecker {
            agent_id: self.agent_id,
            state: HealthCheckerStarted {
                cancel_publisher: health_check_cancel_publisher,
            },
        }
    }
}

impl HealthChecker<HealthCheckerStarted> {
    pub fn stop(self) {
        let _ = self.state.cancel_publisher.publish(()).inspect_err(|err| {
            error!(
                agent_id = %self.agent_id,
                %err ,
                "could not cancel health checker thread"
            );
        });
    }
}
