use crate::agent_control::http_server::status::{Status, SubAgentStatus};
use crate::event::{AgentControlEvent, SubAgentEvent};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;
use tracing::{debug, trace};

pub(super) async fn on_agent_control_event_update_status(
    mut agent_control_event_consumer: UnboundedReceiver<AgentControlEvent>,
    mut sub_agent_event_consumer: UnboundedReceiver<SubAgentEvent>,
    status: Arc<RwLock<Status>>,
) {
    loop {
        tokio::select! {
            Some(agent_control_event) = agent_control_event_consumer.recv() => {
                if let AgentControlEvent::AgentControlStopped = agent_control_event {
                    debug!("status http server agent control stopped event");
                    break;
                }
                update_agent_control_status(agent_control_event, status.clone()).await;
            }
            Some(sub_agent_event) = sub_agent_event_consumer.recv() => {
                update_sub_agent_status(sub_agent_event, status.clone()).await;
            }
            else => {
                debug!("agent_control_event_consumer and sub_agent_event_consumer disconnected");
                break;
            }

        }
    }
}

async fn update_agent_control_status(
    agent_control_event: AgentControlEvent,
    status: Arc<RwLock<Status>>,
) {
    let mut status = status.write().await;
    match agent_control_event {
        AgentControlEvent::AgentControlBecameHealthy(healthy) => {
            debug!("status_http_server event_processor agent_control_became_healthy");
            status.agent_control.healthy(healthy);
        }
        AgentControlEvent::AgentControlBecameUnhealthy(unhealthy) => {
            debug!(
                last_error = unhealthy.last_error(),
                "status_http_server event_processor agent_control_became_unhealthy"
            );
            status.agent_control.unhealthy(unhealthy);
        }
        AgentControlEvent::SubAgentRemoved(agent_id) => {
            status.sub_agents.remove(&agent_id);
        }
        AgentControlEvent::AgentControlStopped => {
            unreachable!("AgentControlStopped is controlled outside");
        }
        AgentControlEvent::OpAMPConnected => {
            trace!("opamp server is reachable");
            status.opamp.reachable();
        }
        AgentControlEvent::OpAMPConnectFailed(error_code, error_message) => {
            debug!(
                error_code,
                error_msg = error_message,
                "opamp server is unreachable"
            );
            status.opamp.unreachable(error_code, error_message);
        }
    }
}
async fn update_sub_agent_status(sub_agent_event: SubAgentEvent, status: Arc<RwLock<Status>>) {
    let mut status = status.write().await;
    match sub_agent_event {
        SubAgentEvent::SubAgentHealthInfo(agent_id, agent_type, health) => {
            if health.is_healthy() {
                debug!(%agent_id, %agent_type, "status_http_server event_processor sub_agent_became_healthy");
            } else {
                debug!(error_msg = health.last_error(), %agent_id, %agent_type, "status_http_server event_processor sub_agent_became_unhealthy");
            }

            status
                .sub_agents
                .entry(agent_id.clone())
                .or_insert_with(|| SubAgentStatus::with_id_and_type(agent_id, agent_type))
                .update_health(health);
        }
    }
}

#[cfg(test)]
mod tests {
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

    use crate::agent_control::config::{AgentID, AgentTypeFQN};
    use crate::agent_control::http_server::status::{
        AgentControlStatus, OpAMPStatus, Status, SubAgentStatus, SubAgentsStatus,
    };
    use crate::agent_control::http_server::status_updater::{
        on_agent_control_event_update_status, update_agent_control_status, update_sub_agent_status,
    };
    use crate::event::AgentControlEvent;
    use crate::event::AgentControlEvent::{
        AgentControlBecameHealthy, AgentControlBecameUnhealthy, AgentControlStopped,
        OpAMPConnectFailed, SubAgentRemoved,
    };
    use crate::event::SubAgentEvent;
    use crate::event::SubAgentEvent::SubAgentHealthInfo;
    use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
    use crate::sub_agent::health::with_start_time::HealthWithStartTime;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_events() {
        struct Test {
            _name: &'static str,
            agent_control_event: Option<AgentControlEvent>,
            sub_agent_event: Option<SubAgentEvent>,
            current_status: Arc<RwLock<Status>>,
            expected_status: Status,
        }
        impl Test {
            async fn run(&self) {
                if let Some(agent_control_event) = self.agent_control_event.clone() {
                    update_agent_control_status(agent_control_event, self.current_status.clone())
                        .await;
                }
                if let Some(sub_agent_event) = self.sub_agent_event.clone() {
                    update_sub_agent_status(sub_agent_event, self.current_status.clone()).await;
                }
                let st = self.current_status.read().await;
                assert_eq!(self.expected_status, *st);
            }
        }

        // Generate stubs. We'll use this to assert on what doesn't need to change
        // in the events
        let opamp_status_random = opamp_status_random();
        let agent_control_status_random = agent_control_status_random();
        let sub_agents_status_random = sub_agents_status_random();

        let tests = vec![
            Test {
                _name: "Unhealthy Agent Control becomes healthy",
                agent_control_event: Some(AgentControlBecameHealthy(Healthy::new(
                    "some status".to_string(),
                ))),
                sub_agent_event: None,
                current_status: Arc::new(RwLock::new(Status {
                    agent_control: AgentControlStatus::new_unhealthy(
                        String::from("some status"),
                        String::from("some error"),
                    ),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    agent_control: AgentControlStatus::new_healthy(String::from("some status")),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                },
            },
            Test {
                _name: "Healthy Agent Control becomes unhealthy",
                agent_control_event: Some(AgentControlBecameUnhealthy(Unhealthy::new(
                    "some status".to_string(),
                    "some error message for agent control unhealthy".to_string(),
                ))),
                sub_agent_event: None,
                current_status: Arc::new(RwLock::new(Status {
                    agent_control: AgentControlStatus::new_healthy(String::from("some status")),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    agent_control: AgentControlStatus::new_unhealthy(
                        String::from("some status"),
                        String::from("some error message for agent control unhealthy"),
                    ),
                    opamp: opamp_status_random.clone(),
                    sub_agents: sub_agents_status_random.clone(),
                },
            },
            Test {
                _name: "Sub Agent first healthy event should add it to the list",
                agent_control_event: None,
                sub_agent_event: Some(SubAgentHealthInfo(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                    HealthWithStartTime::new(Healthy::default().into(), SystemTime::UNIX_EPOCH),
                )),
                current_status: Arc::new(RwLock::new(Status {
                    agent_control: agent_control_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    agent_control: agent_control_status_random.clone(),
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
                agent_control_event: None,
                sub_agent_event: Some(SubAgentHealthInfo(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                    HealthWithStartTime::new(
                        Unhealthy::default()
                            .with_last_error("this is an error message".to_string())
                            .into(),
                        SystemTime::UNIX_EPOCH,
                    ),
                )),
                current_status: Arc::new(RwLock::new(Status {
                    agent_control: agent_control_status_random.clone(),
                    opamp: opamp_status_random.clone(),
                    sub_agents: SubAgentsStatus::default(),
                })),
                expected_status: Status {
                    agent_control: agent_control_status_random.clone(),
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
                agent_control_event: None,
                sub_agent_event: Some(SubAgentHealthInfo(
                    AgentID::new("some-agent-id").unwrap(),
                    AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                    HealthWithStartTime::new(
                        Unhealthy::default()
                            .with_last_error("this is an error message".to_string())
                            .into(),
                        SystemTime::UNIX_EPOCH,
                    ),
                )),
                current_status: Arc::new(RwLock::new(Status {
                    agent_control: agent_control_status_random.clone(),
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
                    agent_control: agent_control_status_random.clone(),
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
                agent_control_event: Some(SubAgentRemoved(AgentID::new("some-agent-id").unwrap())),
                sub_agent_event: None,
                current_status: Arc::new(RwLock::new(Status {
                    agent_control: agent_control_status_random.clone(),
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
                    agent_control: agent_control_status_random.clone(),
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
                agent_control_event: Some(OpAMPConnectFailed(
                    Some(404),
                    String::from("some error msg"),
                )),
                sub_agent_event: None,
                current_status: Arc::new(RwLock::new(Status {
                    agent_control: agent_control_status_random.clone(),
                    opamp: OpAMPStatus::enabled_and_reachable(Some(
                        Url::try_from("http://127.0.0.1").unwrap(),
                    )),
                    sub_agents: sub_agents_status_random.clone(),
                })),
                expected_status: Status {
                    agent_control: agent_control_status_random.clone(),
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

    // create random Agent Control status
    fn agent_control_status_random() -> AgentControlStatus {
        let healthy = en::Boolean(50).fake::<bool>();

        //random status
        let status = Word().fake::<String>();

        if healthy {
            AgentControlStatus::new_healthy(status.clone())
        } else {
            AgentControlStatus::new_unhealthy(status, Words(3..5).fake::<Vec<String>>().join(" "))
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
    #[should_panic(expected = "AgentControlStopped is controlled outside")]
    async fn test_agent_control_stop() {
        update_agent_control_status(
            AgentControlStopped,
            Arc::new(RwLock::new(Status::default())),
        )
        .await;
    }

    #[tokio::test]
    async fn test_event_process_end() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<AgentControlEvent>();
        let (_suba_event_publisher, suba_event_consumer) = unbounded_channel::<SubAgentEvent>();

        let publisher_handle = rt.spawn(async move {
            sleep(Duration::from_millis(10)).await;
            sa_event_publisher.send(AgentControlStopped).unwrap();
        });

        // Then the event will be consumed
        on_agent_control_event_update_status(
            sa_event_consumer,
            suba_event_consumer,
            Arc::new(RwLock::new(Status::default())),
        )
        .await;
        publisher_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_all_channels_closed() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<AgentControlEvent>();
        let (suba_event_publisher, suba_event_consumer) = unbounded_channel::<SubAgentEvent>();

        // We drop the publisher so the channels get disconnected
        drop(sa_event_publisher);
        drop(suba_event_publisher);

        // Then the event will be consumed
        on_agent_control_event_update_status(
            sa_event_consumer,
            suba_event_consumer,
            Arc::new(RwLock::new(Status::default())),
        )
        .await;
    }
}
