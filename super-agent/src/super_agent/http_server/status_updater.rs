use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;
use tracing::debug;

use crate::event::SuperAgentEvent;
use crate::super_agent::http_server::status::{Status, SubAgentStatus};

pub(super) async fn on_super_agent_event_update_status(
    mut sa_event_consumer: UnboundedReceiver<SuperAgentEvent>,
    status: Arc<RwLock<Status>>,
) {
    while let Some(super_agent_event) = sa_event_consumer.recv().await {
        if let SuperAgentEvent::SuperAgentStopped = super_agent_event {
            debug!("status http server super agent stopped event");
            break;
        }
        update_status(super_agent_event, status.clone()).await;
    }
}

async fn update_status(super_agent_event: SuperAgentEvent, status: Arc<RwLock<Status>>) {
    let mut status = status.write().await;
    match super_agent_event {
        SuperAgentEvent::SuperAgentBecameHealthy => {
            debug!("status_http_server event_processor super_agent_became_healthy");
            status.super_agent.healthy = true;
            status.super_agent.last_error = String::default();
        }
        SuperAgentEvent::SuperAgentBecameUnhealthy(error_msg) => {
            debug!(
                error_msg,
                "status_http_server event_processor super_agent_became_unhealthy"
            );
            status.super_agent.healthy = false;
            status.super_agent.last_error = error_msg;
        }
        SuperAgentEvent::SubAgentBecameUnhealthy(agent_id, agent_type, error_msg) => {
            debug!(error_msg, %agent_id, %agent_type, "status_http_server event_processor sub_agent_became_unhealthy");
            status
                .sub_agents
                .entry(agent_id.clone())
                .or_insert_with(|| SubAgentStatus::new(agent_id, agent_type))
                .unhealthy(error_msg);
        }
        SuperAgentEvent::SubAgentBecameHealthy(agent_id, agent_type) => {
            debug!(%agent_id, %agent_type, "status_http_server event_processor sub_agent_became_healthy");
            status
                .sub_agents
                .entry(agent_id.clone())
                .or_insert_with(|| SubAgentStatus::new(agent_id, agent_type))
                .healthy();
        }
        SuperAgentEvent::SubAgentRemoved(agent_id) => {
            status.sub_agents.remove(&agent_id);
        }
        SuperAgentEvent::SuperAgentStopped => {
            unreachable!("SuperAgentStopped is controlled outside");
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::runtime::Handle;
    use tokio::sync::mpsc::unbounded_channel;

    use tokio::sync::RwLock;
    use tokio::time::sleep;

    use SuperAgentEvent::{SubAgentBecameHealthy, SubAgentBecameUnhealthy};

    use crate::event::SuperAgentEvent;
    use crate::event::SuperAgentEvent::{
        SubAgentRemoved, SuperAgentBecameHealthy, SuperAgentBecameUnhealthy, SuperAgentStopped,
    };
    use crate::opamp::LastErrorMessage;
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::super_agent::http_server::status::{
        OpAMPStatus, Status, SubAgentStatus, SubAgentsStatus, SuperAgentStatus,
    };
    use crate::super_agent::http_server::status_updater::{
        on_super_agent_event_update_status, update_status,
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn test_events() {
        struct Test {
            _name: &'static str,
            super_agent_event: SuperAgentEvent,
            current_status: Arc<RwLock<Status>>,
            expected_status: Status,
        }
        impl Test {
            async fn run(&self) {
                update_status(self.super_agent_event.clone(), self.current_status.clone()).await;
                let st = self.current_status.read().await;
                assert_eq!(self.expected_status, *st);
            }
        }

        let tests = vec![
            Test {
                _name: "Unhealthy Super Agent becomes healthy",
                super_agent_event: SuperAgentBecameHealthy,
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus {
                        healthy: false,
                        status: String::from("some status"),
                        last_error: String::from("some error"),
                    },
                    opamp: OpAMPStatus {
                        endpoint: String::from("some endpoint"),
                        reachable: true,
                        enabled: true,
                    },
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus {
                        healthy: true,
                        status: String::from("some status"),
                        last_error: String::default(),
                    },
                    opamp: OpAMPStatus {
                        endpoint: String::from("some endpoint"),
                        reachable: true,
                        enabled: true,
                    },
                    sub_agents: SubAgentsStatus::default(),
                },
            },
            Test {
                _name: "Healthy Super Agent becomes unhealthy",
                super_agent_event: SuperAgentBecameUnhealthy(String::from(
                    "some error message for super agent unhealthy",
                )),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus {
                        healthy: true,
                        status: String::default(),
                        last_error: String::default(),
                    },
                    opamp: OpAMPStatus {
                        endpoint: String::from("some endpoint"),
                        reachable: false,
                        enabled: true,
                    },
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus {
                        healthy: false,
                        status: String::default(),
                        last_error: String::from("some error message for super agent unhealthy"),
                    },
                    opamp: OpAMPStatus {
                        endpoint: String::from("some endpoint"),
                        reachable: false,
                        enabled: true,
                    },
                    sub_agents: SubAgentsStatus::default(),
                },
            },
            Test {
                _name: "Sub Agent first healthy event should add it to the list",
                super_agent_event: SubAgentBecameHealthy(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::from("some-agent-type"),
                ),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([(
                        AgentID::new("some-agent-id").unwrap(),
                        SubAgentStatus::create(
                            AgentID::new("some-agent-id").unwrap(),
                            AgentTypeFQN::from("some-agent-type"),
                            true,
                            String::default(),
                        ),
                    )])),
                },
            },
            Test {
                _name: "Sub Agent first unhealthy event should add it to the list",
                super_agent_event: SubAgentBecameUnhealthy(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::from("some-agent-type"),
                    LastErrorMessage::from("this is an error message"),
                ),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([(
                        AgentID::new("some-agent-id").unwrap(),
                        SubAgentStatus::create(
                            AgentID::new("some-agent-id").unwrap(),
                            AgentTypeFQN::from("some-agent-type"),
                            false,
                            String::from("this is an error message"),
                        ),
                    )])),
                },
            },
            Test {
                _name: "Sub Agent second unhealthy event should change existing one",
                super_agent_event: SubAgentBecameUnhealthy(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::from("some-agent-type"),
                    LastErrorMessage::from("this is an error message"),
                ),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([
                        (
                            AgentID::new("some-agent-id").unwrap(),
                            SubAgentStatus::create(
                                AgentID::new("some-agent-id").unwrap(),
                                AgentTypeFQN::from("some-agent-type"),
                                true,
                                String::default(),
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::create(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::from("some-other-type"),
                                true,
                                String::default(),
                            ),
                        ),
                    ])),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([
                        (
                            AgentID::new("some-agent-id").unwrap(),
                            SubAgentStatus::create(
                                AgentID::new("some-agent-id").unwrap(),
                                AgentTypeFQN::from("some-agent-type"),
                                false,
                                String::from("this is an error message"),
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::create(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::from("some-other-type"),
                                true,
                                String::default(),
                            ),
                        ),
                    ])),
                },
            },
            Test {
                _name: "Sub Agent gets removed",
                super_agent_event: SubAgentRemoved(AgentID::new("some-agent-id").unwrap()),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([
                        (
                            AgentID::new("some-agent-id").unwrap(),
                            SubAgentStatus::create(
                                AgentID::new("some-agent-id").unwrap(),
                                AgentTypeFQN::from("some-agent-type"),
                                true,
                                String::default(),
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::create(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::from("some-other-type"),
                                true,
                                String::default(),
                            ),
                        ),
                    ])),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::default(),
                    opamp: OpAMPStatus::default(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([(
                        AgentID::new("some-other-id").unwrap(),
                        SubAgentStatus::create(
                            AgentID::new("some-other-id").unwrap(),
                            AgentTypeFQN::from("some-other-type"),
                            true,
                            String::default(),
                        ),
                    )])),
                },
            },
        ];

        for test in tests {
            test.run().await;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[should_panic(expected = "SuperAgentStopped is controlled outside")]
    async fn test_super_agent_stop() {
        update_status(SuperAgentStopped, Arc::new(RwLock::new(Status::default()))).await;
    }

    #[tokio::test]
    async fn test_event_process_end() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<SuperAgentEvent>();

        let publisher_handle = rt.spawn(async move {
            sleep(Duration::from_millis(10)).await;
            sa_event_publisher.send(SuperAgentStopped).unwrap();
        });

        // Then the event will be consumed
        on_super_agent_event_update_status(
            sa_event_consumer,
            Arc::new(RwLock::new(Status::default())),
        )
        .await;
        publisher_handle.await.unwrap();
    }
}
