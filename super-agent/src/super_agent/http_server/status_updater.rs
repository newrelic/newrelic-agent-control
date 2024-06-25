use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;
use tracing::debug;

use crate::event::SuperAgentEvent;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
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
        SuperAgentEvent::SuperAgentBecameHealthy(healthy) => {
            debug!("status_http_server event_processor super_agent_became_healthy");
            status.super_agent.healthy(healthy);
        }
        SuperAgentEvent::SuperAgentBecameUnhealthy(unhealthy) => {
            debug!(
                last_error = unhealthy.last_error(),
                "status_http_server event_processor super_agent_became_unhealthy"
            );
            status.super_agent.unhealthy(unhealthy);
        }
        SuperAgentEvent::SubAgentBecameUnhealthy(agent_id, agent_type, unhealthy, start_time) => {
            debug!(error_msg = unhealthy.last_error(), %agent_id, %agent_type, "status_http_server event_processor sub_agent_became_unhealthy");
            status
                .sub_agents
                .entry(agent_id.clone())
                .or_insert_with(|| SubAgentStatus::with_id_and_type(agent_id, agent_type))
                .update_health(HealthWithStartTime::new(unhealthy.into(), start_time));
        }
        SuperAgentEvent::SubAgentBecameHealthy(agent_id, agent_type, healthy, start_time) => {
            debug!(%agent_id, %agent_type, "status_http_server event_processor sub_agent_became_healthy");
            status
                .sub_agents
                .entry(agent_id.clone())
                .or_insert_with(|| SubAgentStatus::with_id_and_type(agent_id, agent_type))
                .update_health(HealthWithStartTime::new(healthy.into(), start_time));
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
    use std::time::{Duration, SystemTime};

    use fake::faker::boolean::en;
    use fake::faker::filesystem::en::Semver;
    use fake::faker::lorem::en::{Word, Words};
    use fake::{Fake, Faker};
    use tokio::runtime::Handle;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::sync::RwLock;
    use tokio::time::sleep;

    use url::Url;
    use SuperAgentEvent::{SubAgentBecameHealthy, SubAgentBecameUnhealthy};

    use crate::event::SuperAgentEvent;
    use crate::event::SuperAgentEvent::{
        OpAMPConnectFailed, SubAgentRemoved, SuperAgentBecameHealthy, SuperAgentBecameUnhealthy,
        SuperAgentStopped,
    };
    use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
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
                super_agent_event: SuperAgentBecameHealthy(Healthy::new("some status".to_string())),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::new_unhealthy(
                        String::from("some status"),
                        String::from("some error"),
                    ),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::new_healthy(String::from("some status")),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                },
            },
            Test {
                _name: "Healthy Super Agent becomes unhealthy",
                super_agent_event: SuperAgentBecameUnhealthy(Unhealthy::new(
                    "some status".to_string(),
                    "some error message for super agent unhealthy".to_string(),
                )),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: SuperAgentStatus::new_healthy(String::from("some status")),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    super_agent: SuperAgentStatus::new_unhealthy(
                        String::from("some status"),
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
                    AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                    Healthy::default(),
                    SystemTime::UNIX_EPOCH,
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
                            AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                            String::default(),
                            true,
                            None,
                            0,
                            0,
                        ),
                    )])),
                },
            },
            Test {
                _name: "Sub Agent first unhealthy event should add it to the list",
                super_agent_event: SubAgentBecameUnhealthy(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                    Unhealthy::default().with_last_error("this is an error message".to_string()),
                    SystemTime::UNIX_EPOCH,
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
                            AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                            String::default(),
                            false,
                            Some(String::from("this is an error message")),
                            0,
                            0,
                        ),
                    )])),
                },
            },
            Test {
                _name: "Sub Agent second unhealthy event should change existing one",
                super_agent_event: SubAgentBecameUnhealthy(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                    Unhealthy::default().with_last_error("this is an error message".to_string()),
                    SystemTime::UNIX_EPOCH,
                ),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::from(HashMap::from([
                        (
                            AgentID::new("some-agent-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-agent-id").unwrap(),
                                AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                                String::default(),
                                true,
                                Some(String::default()),
                                0,
                                0,
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                                String::default(),
                                true,
                                Some(String::default()),
                                0,
                                0,
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
                                AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                                String::default(),
                                false,
                                Some(String::from("this is an error message")),
                                0,
                                0,
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                                String::default(),
                                true,
                                Some(String::default()),
                                0,
                                0,
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
                                AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                                String::default(),
                                true,
                                Some(String::default()),
                                0,
                                0,
                            ),
                        ),
                        (
                            AgentID::new("some-other-id").unwrap(),
                            SubAgentStatus::new(
                                AgentID::new("some-other-id").unwrap(),
                                AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                                String::default(),
                                true,
                                Some(String::default()),
                                0,
                                0,
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
                            AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                            String::default(),
                            true,
                            Some(String::default()),
                            0,
                            0,
                        ),
                    )])),
                },
            },
            Test {
                _name: "OpAMP Agent gets unhealthy",
                super_agent_event: OpAMPConnectFailed(Some(404), String::from("some error msg")),
                current_status: Arc::new(RwLock::new(Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: OpAMPStatus::enabled_and_reachable(Some(
                        Url::try_from("http://127.0.0.1").unwrap(),
                    )),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    super_agent: super_agent_status_random.clone(),
                    opamp: OpAMPStatus::enabled_and_unreachable(
                        Some(Url::try_from("http://127.0.0.1").unwrap()),
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

    fn uri_to_url(uri: http::Uri) -> Option<Url> {
        let uri_str = uri.to_string();
        Url::try_from(uri_str.as_str()).ok()
    }

    // create random OpAMP status
    fn opamp_status_random() -> OpAMPStatus {
        // There is no fake instance for the `Url` type, so we will assemble it step by step from an `Uri`,
        // given that all URLs are URIs but not all URIs are URLs.
        let endpoint = uri_to_url(Faker.fake::<http::Uri>());
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
        let status = Word().fake::<String>();

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
        let agent_type_fqn = format!(
            "{}/{}:{}",
            Word().fake::<&str>(),
            Word().fake::<&str>(),
            Semver().fake::<String>(),
        );
        let agent_type = AgentTypeFQN::try_from(agent_type_fqn.as_str()).unwrap();
        //random status
        let status = Word().fake::<String>();

        SubAgentStatus::new(agent_id, agent_type, status, healthy, last_error, 0, 0)
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
