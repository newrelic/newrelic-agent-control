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
            status.super_agent.healthy();
        }
        SuperAgentEvent::SuperAgentBecameUnhealthy(error_msg) => {
            debug!(
                error_msg,
                "status_http_server event_processor super_agent_became_unhealthy"
            );
            status.super_agent.unhealthy(error_msg);
        }
        SuperAgentEvent::SubAgentBecameUnhealthy(agent_id, agent_type, error_msg) => {
            debug!(error_msg, %agent_id, %agent_type, "status_http_server event_processor sub_agent_became_unhealthy");
            status
                .sub_agents
                .entry(agent_id.clone())
                .or_insert_with(|| SubAgentStatus::with_id_and_type(agent_id, agent_type))
                .unhealthy(error_msg);
        }
        SuperAgentEvent::SubAgentBecameHealthy(agent_id, agent_type) => {
            debug!(%agent_id, %agent_type, "status_http_server event_processor sub_agent_became_healthy");
            status
                .sub_agents
                .entry(agent_id.clone())
                .or_insert_with(|| SubAgentStatus::with_id_and_type(agent_id, agent_type))
                .healthy();
        }
        SuperAgentEvent::SubAgentRemoved(agent_id) => {
            status.sub_agents.remove(&agent_id);
        }
        SuperAgentEvent::SuperAgentStopped => {
            unreachable!("SuperAgentStopped is controlled outside");
        }
        SuperAgentEvent::OpAMPConnected => {
            debug!("opamp server is reachable");
            status.opamp.reachable();
        }
        SuperAgentEvent::OpAMPConnectFailed(error_code, error_message) => {
            debug!(
                error_code,
                error_msg = error_message,
                "opamp server is unreachable"
            );
            status.opamp.unreachable(error_code, error_message);
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use fake::faker::boolean::en;
    use fake::faker::lorem::en::{Word, Words};
    use fake::{Fake, Faker};
    use tokio::runtime::Handle;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::sync::RwLock;
    use tokio::time::sleep;

    use SuperAgentEvent::{SubAgentBecameHealthy, SubAgentBecameUnhealthy};

    use crate::event::SuperAgentEvent;
    use crate::event::SuperAgentEvent::{
        OpAMPConnectFailed, SubAgentRemoved, SuperAgentBecameHealthy, SuperAgentBecameUnhealthy,
        SuperAgentStopped,
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

        // Generate stubs. We'll use this to assert on what doesn't need to change
        // in the events
        let opamp_status_random = opamp_status_random();
        let super_agent_status_random = super_agent_status_random();
        let sub_agents_status_random = sub_agents_status_random();

        let tests = vec![
            Test {
                _name: "Unhealthy Super Agent becomes healthy",
                super_agent_event: SuperAgentBecameHealthy,
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::new_unhealthy(
                        Some(String::from("some status")),
                        String::from("some error"),
                    ),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::new_healthy(Some(String::from("some status"))),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                },
            },
            Test {
                _name: "Healthy Super Agent becomes unhealthy",
                super_agent_event: SuperAgentBecameUnhealthy(String::from(
                    "some error message for super agent unhealthy",
                )),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::new_healthy(Some(String::from("some status"))),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::new_unhealthy(
                        Some(String::from("some status")),
                        String::from("some error message for super agent unhealthy"),
                    ),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                },
            },
            Test {
                _name: "Sub Agent first healthy event should add it to the list",
                super_agent_event: SubAgentBecameHealthy(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::from("some-agent-type"),
                ),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([(
                        AgentID::new("some-agent-id").unwrap(),
                        SubAgentStatus::new(
                            AgentID::new("some-agent-id").unwrap(),
                            AgentTypeFQN::from("some-agent-type"),
                            None,
                            true,
                            None,
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
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([(
                        AgentID::new("some-agent-id").unwrap(),
                        SubAgentStatus::new(
                            AgentID::new("some-agent-id").unwrap(),
                            AgentTypeFQN::from("some-agent-type"),
                            None,
                            false,
                            Some(String::from("this is an error message")),
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
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([
                        (
                            AgentID::new("some-agent-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-agent-id").unwrap(),
                                AgentTypeFQN::from("some-agent-type"),
                                None,
                                true,
                                Some(String::default()),
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::from("some-other-type"),
                                None,
                                true,
                                Some(String::default()),
                            ),
                        ),
                    ])),
                })),
                expected_status: Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([
                        (
                            AgentID::new("some-agent-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-agent-id").unwrap(),
                                AgentTypeFQN::from("some-agent-type"),
                                None,
                                false,
                                Some(String::from("this is an error message")),
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::from("some-other-type"),
                                None,
                                true,
                                Some(String::default()),
                            ),
                        ),
                    ])),
                },
            },
            Test {
                _name: "Sub Agent gets removed",
                super_agent_event: SubAgentRemoved(AgentID::new("some-agent-id").unwrap()),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([
                        (
                            AgentID::new("some-agent-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-agent-id").unwrap(),
                                AgentTypeFQN::from("some-agent-type"),
                                None,
                                true,
                                Some(String::default()),
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::from("some-other-type"),
                                None,
                                true,
                                Some(String::default()),
                            ),
                        ),
                    ])),
                })),
                expected_status: Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random,
                    sub_agents: SubAgentsStatus::from(HashMap::from([(
                        AgentID::new("some-other-id").unwrap(),
                        SubAgentStatus::new(
                            AgentID::new("some-other-id").unwrap(),
                            AgentTypeFQN::from("some-other-type"),
                            None,
                            true,
                            Some(String::default()),
                        ),
                    )])),
                },
            },
            Test {
                _name: "OpAMP Agent gets unhealthy",
                super_agent_event: OpAMPConnectFailed(Some(404), String::from("some error msg")),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: OpAMPStatus::enabled_and_reachable(Some(String::from("some-endpoint"))),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: OpAMPStatus::enabled_and_unreachable(
                        Some(String::from("some-endpoint")),
                        404,
                        String::from("some error msg"),
                    ),
                    sub_agents: sub_agents_status_random.clone(),
                },
            },
        ];

        for test in tests {
            test.run().await;
        }
    }

    // create random OpAMP status
    fn opamp_status_random() -> OpAMPStatus {
        let endpoint = Some(Faker.fake::<http::Uri>().to_string());
        let reachable = en::Boolean(50).fake::<bool>();
        let enabled = en::Boolean(50).fake::<bool>();
        let error_code = Some((400..599).fake::<u16>());
        let error_message = Some(Words(3..5).fake::<Vec<String>>().join(" "));

        OpAMPStatus::new(enabled, endpoint, reachable, error_code, error_message)
    }

    // create random Super Agent status
    fn super_agent_status_random() -> SuperAgentStatus {
        let healthy = en::Boolean(50).fake::<bool>();

        //random status
        let status = en::Boolean(50)
            .fake::<bool>()
            .then_some(Word().fake::<String>())
            .or(None);

        if healthy {
            SuperAgentStatus::new_healthy(status.clone())
        } else {
            SuperAgentStatus::new_unhealthy(status, Words(3..5).fake::<Vec<String>>().join(" "))
        }
    }

    // create random Sub Agent status
    fn sub_agent_status_random() -> SubAgentStatus {
        let healthy = en::Boolean(50).fake::<bool>();
        let last_error = healthy
            .then_some(Words(3..5).fake::<Vec<String>>().join(" "))
            .or(Some(String::default()));
        let agent_id = AgentID::new(Word().fake::<&str>()).unwrap();
        let agent_type = AgentTypeFQN::from(Word().fake::<&str>());
        //random status
        let status = en::Boolean(50)
            .fake::<bool>()
            .then_some(Word().fake::<String>())
            .or(None);

        SubAgentStatus::new(agent_id, agent_type, status, healthy, last_error)
    }

    // create N (0..5) random Sub Agent status
    fn sub_agents_status_random() -> SubAgentsStatus {
        let sub_agents_amount = (0..5).fake::<u32>();
        let mut sub_agents: HashMap<AgentID, SubAgentStatus> = HashMap::new();
        for _ in 0..sub_agents_amount {
            let sub_agent = sub_agent_status_random();
            sub_agents.insert(sub_agent.agent_id(), sub_agent);
        }
        SubAgentsStatus::from(sub_agents)
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
