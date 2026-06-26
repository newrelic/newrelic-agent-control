//! Event loop that updates the shared [`Status`] from Agent Control and sub-agent events.

use crate::agent_control::http_server::status::{Status, SubAgentStatus, build_agent_attributes};
use crate::event::{AgentControlEvent, SubAgentEvent};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{debug, trace, warn};

pub(super) async fn on_agent_control_event_update_status(
    mut agent_control_event_consumer: UnboundedReceiver<AgentControlEvent>,
    mut sub_agent_event_consumer: UnboundedReceiver<SubAgentEvent>,
    status: Arc<RwLock<Status>>,
) {
    loop {
        tokio::select! {
            maybe_agent_control_event = agent_control_event_consumer.recv() => {
                match maybe_agent_control_event {
                    Some(AgentControlEvent::AgentControlStopped) => {
                        debug!("status http server agent control stopped event");
                        break;
                    }
                    Some(agent_control_event) => {
                        update_agent_control_status(agent_control_event, status.clone()).await;
                    }
                    None => {
                        debug!("agent_control_event_consumer disconnected");
                        break;
                    }
                }
            }
            maybe_sub_agent_event = sub_agent_event_consumer.recv() => {
                match maybe_sub_agent_event {
                    Some(sub_agent_event) => {
                        update_sub_agent_status(sub_agent_event, status.clone()).await;
                    }
                    None => {
                        debug!("sub_agent_event_consumer disconnected");
                        break;
                    }
                }
            }
            else => {
                debug!("unexpected condition in status http server event processor");
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
        AgentControlEvent::HealthUpdated(health) => {
            debug!("status_http_server event_processor agent_control_health_updated");
            status.agent_control.set_health(health);
        }
        AgentControlEvent::SubAgentRemoved(agent_id) => {
            status.agents.remove(&agent_id);
        }
        AgentControlEvent::AgentControlStopped => {
            unreachable!("AgentControlStopped is controlled outside");
        }
        AgentControlEvent::OpAMPConnected => {
            trace!("opamp server is reachable");
            status.fleet.reachable();
        }
        AgentControlEvent::OpAMPConnectFailed(error_code, error_message) => {
            debug!(
                error_code,
                error_msg = error_message,
                "opamp server is unreachable"
            );
            status.fleet.unreachable(error_code, error_message);
        }
        AgentControlEvent::AgentDescriptionUpdated(agent_description) => {
            trace!("Setting Agent Control attributes for HTTP-Server");
            let attributes_update = build_agent_attributes(agent_description);
            status.agent_control.attributes.extend(attributes_update);
        }
    }
}

async fn update_sub_agent_status(sub_agent_event: SubAgentEvent, status: Arc<RwLock<Status>>) {
    let mut status = status.write().await;
    match sub_agent_event {
        SubAgentEvent::HealthUpdated(agent_identity, health) => {
            if health.is_healthy() {
                debug!(agent_id = %agent_identity.id, agent_type = %agent_identity.agent_type_id, "status_http_server event_processor sub_agent_became_healthy");
            } else {
                debug!(error_msg = health.last_error(), agent_id = %agent_identity.id, agent_type = %agent_identity.agent_type_id, "status_http_server event_processor sub_agent_became_unhealthy");
            }

            status
                .agents
                .entry(agent_identity.id.clone())
                .or_insert_with(|| {
                    warn!(agent_id = %agent_identity.id,"Event sub_agent_health_info received before sub_agent_started");
                    SubAgentStatus::with_identity(agent_identity).with_start_time(health.start_time())
                }).update_health(health);
        }
        SubAgentEvent::SubAgentStarted(agent_identity, start_time) => {
            status
                .agents
                .entry(agent_identity.id.clone())
                .or_insert_with(|| {
                    SubAgentStatus::with_identity(agent_identity).with_start_time(start_time)
                });
        }
        SubAgentEvent::AgentDescriptionUpdated(agent_identity, agent_description) => {
            trace!("Setting SubAgent attributes for HTTP-Server");
            let attributes_update = build_agent_attributes(agent_description);
            status
                .agents
                .entry(agent_identity.id.clone())
                // New entry if sub-agent was unknown (Eg: SubAgentStarted was never received)
                .or_insert_with(|| SubAgentStatus::with_identity(agent_identity))
                .attributes
                .extend(attributes_update);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use opamp_client::operation::settings::{AgentDescription, DescriptionValueType};
    use rstest::rstest;
    use tokio::runtime::Handle;
    use tokio::sync::RwLock;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::time::sleep;
    use url::Url;

    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::http_server::status::{
        AgentControlStatus, HealthInfo, OpAMPStatus, Status, SubAgentStatus, SubAgentsStatus,
    };
    use crate::agent_control::http_server::status_updater::{
        on_agent_control_event_update_status, update_agent_control_status, update_sub_agent_status,
    };
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::checkers::health::health_checker::{Healthy, Unhealthy};
    use crate::checkers::health::with_start_time::HealthWithStartTime;
    use crate::event::AgentControlEvent;
    use crate::event::AgentControlEvent::{
        AgentControlStopped, OpAMPConnectFailed, SubAgentRemoved,
    };
    use crate::event::SubAgentEvent;
    use crate::event::SubAgentEvent::HealthUpdated;
    use crate::sub_agent::identity::AgentIdentity;

    fn fixed_fleet() -> OpAMPStatus {
        OpAMPStatus::enabled_and_reachable(Some(Url::try_from("http://127.0.0.1").unwrap()))
    }

    fn fixed_agent_control() -> AgentControlStatus {
        AgentControlStatus::new_healthy("running".to_string())
    }

    fn fixed_agents() -> SubAgentsStatus {
        SubAgentsStatus::from([(
            AgentID::try_from("fixed-agent-id").unwrap(),
            SubAgentStatus::new(
                AgentID::try_from("fixed-agent-id").unwrap(),
                AgentTypeID::try_from("ns/type:1.0.0").unwrap(),
                0,
                HealthInfo::new(String::default(), true, None, 0, 0),
            ),
        )])
    }

    fn agent_identity(id: &str) -> AgentIdentity {
        AgentIdentity::from((
            AgentID::try_from(id).unwrap(),
            AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
        ))
    }

    fn agent_description(
        identifying: &[(&str, &str)],
        non_identifying: &[(&str, &str)],
    ) -> AgentDescription {
        AgentDescription {
            identifying_attributes: identifying
                .iter()
                .map(|(k, v)| (k.to_string(), DescriptionValueType::String(v.to_string())))
                .collect(),
            non_identifying_attributes: non_identifying
                .iter()
                .map(|(k, v)| (k.to_string(), DescriptionValueType::String(v.to_string())))
                .collect(),
        }
    }

    #[rstest]
    #[case::health_updated_becomes_healthy(
        AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::new().with_status("some status".to_string()).into(),
            SystemTime::UNIX_EPOCH,
        )),
        Status {
            agent_control: AgentControlStatus::new_unhealthy(
                "some status".to_string(),
                "some error".to_string(),
            ),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
        Status {
            agent_control: AgentControlStatus::new_healthy("some status".to_string()),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
    )]
    #[case::health_updated_becomes_unhealthy(
        AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Unhealthy::new("some error message for agent control unhealthy".to_string())
                .with_status("some status".to_string())
                .into(),
            SystemTime::UNIX_EPOCH,
        )),
        Status {
            agent_control: AgentControlStatus::new_healthy("some status".to_string()),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
        Status {
            agent_control: AgentControlStatus::new_unhealthy(
                "some status".to_string(),
                "some error message for agent control unhealthy".to_string(),
            ),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
    )]
    #[case::sub_agent_removed(
        SubAgentRemoved(AgentID::try_from("some-agent-id").unwrap()),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([
                (
                    AgentID::try_from("some-agent-id").unwrap(),
                    SubAgentStatus::new(
                        AgentID::try_from("some-agent-id").unwrap(),
                        AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                        0,
                        HealthInfo::new(String::default(), true, None, 0, 0),
                    ),
                ),
                (
                    AgentID::try_from("some-other-id").unwrap(),
                    SubAgentStatus::new(
                        AgentID::try_from("some-other-id").unwrap(),
                        AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                        0,
                        HealthInfo::new(String::default(), true, None, 0, 0),
                    ),
                ),
            ]),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-other-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-other-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                ),
            )]),
        },
    )]
    #[case::opamp_connected(
        AgentControlEvent::OpAMPConnected,
        Status {
            agent_control: fixed_agent_control(),
            fleet: OpAMPStatus::enabled_and_unreachable(
                Some(Url::try_from("http://127.0.0.1").unwrap()),
                503,
                "service unavailable".to_string(),
            ),
            agents: fixed_agents(),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
    )]
    #[case::opamp_connect_failed(
        OpAMPConnectFailed(Some(404), "some error msg".to_string()),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: OpAMPStatus::enabled_and_unreachable(
                Some(Url::try_from("http://127.0.0.1").unwrap()),
                404,
                "some error msg".to_string(),
            ),
            agents: fixed_agents(),
        },
    )]
    #[case::agent_description_updated_sets_new_attributes(
        AgentControlEvent::AgentDescriptionUpdated(agent_description(
            &[("agent_version", "1.0.0")],
            &[("host_name", "my-host")],
        )),
        Status {
            agent_control: AgentControlStatus::new_healthy("running".to_string()),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
        Status {
            agent_control: AgentControlStatus::new_healthy("running".to_string()).with_attributes(
                HashMap::from([
                    ("identifying/agent_version".to_string(), "1.0.0".to_string()),
                    ("non-identifying/host_name".to_string(), "my-host".to_string()),
                ]),
            ),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
    )]
    #[case::agent_description_updated_extends_and_preserves_existing_attributes(
        AgentControlEvent::AgentDescriptionUpdated(agent_description(
            &[("agent_version", "2.0.0")],
            &[("new_key", "new_val")],
        )),
        Status {
            agent_control: AgentControlStatus::new_healthy("running".to_string()).with_attributes(
                HashMap::from([
                    ("identifying/agent_version".to_string(), "1.0.0".to_string()),
                    ("non-identifying/host_name".to_string(), "my-host".to_string()),
                ]),
            ),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
        Status {
            agent_control: AgentControlStatus::new_healthy("running".to_string()).with_attributes(
                HashMap::from([
                    ("identifying/agent_version".to_string(), "2.0.0".to_string()), // overwritten
                    ("non-identifying/host_name".to_string(), "my-host".to_string()), // preserved
                    ("non-identifying/new_key".to_string(), "new_val".to_string()),  // added
                ]),
            ),
            fleet: fixed_fleet(),
            agents: fixed_agents(),
        },
    )]
    #[tokio::test]
    async fn test_update_agent_control_status(
        #[case] event: AgentControlEvent,
        #[case] current_status: Status,
        #[case] expected_status: Status,
    ) {
        let status = Arc::new(RwLock::new(current_status));
        update_agent_control_status(event, status.clone()).await;
        assert_eq!(expected_status, *status.read().await);
    }

    #[rstest]
    #[case::health_updated_first_healthy(
        HealthUpdated(
            agent_identity("some-agent-id"),
            HealthWithStartTime::new(Healthy::default().into(), SystemTime::UNIX_EPOCH),
        ),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::default(),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                ),
            )]),
        },
    )]
    #[case::health_updated_first_unhealthy(
        HealthUpdated(
            agent_identity("some-agent-id"),
            HealthWithStartTime::new(
                Unhealthy::default()
                    .with_last_error("this is an error message".to_string())
                    .into(),
                SystemTime::UNIX_EPOCH,
            ),
        ),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::default(),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(
                        String::default(),
                        false,
                        Some("this is an error message".to_string()),
                        0,
                        0,
                    ),
                ),
            )]),
        },
    )]
    #[case::health_updated_changes_existing_agent(
        HealthUpdated(
            agent_identity("some-agent-id"),
            HealthWithStartTime::new(
                Unhealthy::default()
                    .with_last_error("this is an error message".to_string())
                    .into(),
                SystemTime::UNIX_EPOCH,
            ),
        ),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([
                (
                    AgentID::try_from("some-agent-id").unwrap(),
                    SubAgentStatus::new(
                        AgentID::try_from("some-agent-id").unwrap(),
                        AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                        0,
                        HealthInfo::new(String::default(), true, None, 0, 0),
                    ),
                ),
                (
                    AgentID::try_from("some-other-id").unwrap(),
                    SubAgentStatus::new(
                        AgentID::try_from("some-other-id").unwrap(),
                        AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                        0,
                        HealthInfo::new(String::default(), true, None, 0, 0),
                    ),
                ),
            ]),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([
                (
                    AgentID::try_from("some-agent-id").unwrap(),
                    SubAgentStatus::new(
                        AgentID::try_from("some-agent-id").unwrap(),
                        AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                        0,
                        HealthInfo::new(
                            String::default(),
                            false,
                            Some("this is an error message".to_string()),
                            0,
                            0,
                        ),
                    ),
                ),
                (
                    AgentID::try_from("some-other-id").unwrap(),
                    SubAgentStatus::new(
                        AgentID::try_from("some-other-id").unwrap(),
                        AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                        0,
                        HealthInfo::new(String::default(), true, None, 0, 0),
                    ),
                ),
            ]),
        },
    )]
    #[case::sub_agent_started_adds_new_agent(
        SubAgentEvent::SubAgentStarted(agent_identity("some-agent-id"), SystemTime::UNIX_EPOCH),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::default(),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::with_identity(agent_identity("some-agent-id"))
                    .with_start_time(SystemTime::UNIX_EPOCH),
            )]),
        },
    )]
    #[case::sub_agent_started_does_not_override_existing(
        SubAgentEvent::SubAgentStarted(agent_identity("some-agent-id"), SystemTime::UNIX_EPOCH),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                ),
            )]),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                ),
            )]),
        },
    )]
    #[case::agent_description_updated_sets_attributes_on_known_agent(
        SubAgentEvent::AgentDescriptionUpdated(
            agent_identity("some-agent-id"),
            agent_description(&[("agent_version", "1.0.0")], &[("host_name", "my-host")]),
        ),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                ),
            )]),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                )
                .with_attributes(HashMap::from([
                    ("identifying/agent_version".to_string(), "1.0.0".to_string()),
                    ("non-identifying/host_name".to_string(), "my-host".to_string()),
                ])),
            )]),
        },
    )]
    #[case::agent_description_updated_extends_and_preserves_existing_attributes(
        SubAgentEvent::AgentDescriptionUpdated(
            agent_identity("some-agent-id"),
            agent_description(&[("agent_version", "2.0.0")], &[("new_key", "new_val")]),
        ),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                )
                .with_attributes(HashMap::from([
                    ("identifying/agent_version".to_string(), "1.0.0".to_string()),
                    ("non-identifying/host_name".to_string(), "my-host".to_string()),
                ])),
            )]),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::new(
                    AgentID::try_from("some-agent-id").unwrap(),
                    AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                    0,
                    HealthInfo::new(String::default(), true, None, 0, 0),
                )
                .with_attributes(HashMap::from([
                    ("identifying/agent_version".to_string(), "2.0.0".to_string()), // overwritten
                    ("non-identifying/host_name".to_string(), "my-host".to_string()), // preserved
                    ("non-identifying/new_key".to_string(), "new_val".to_string()),  // added
                ])),
            )]),
        },
    )]
    #[case::agent_description_updated_creates_agent_when_absent(
        SubAgentEvent::AgentDescriptionUpdated(
            agent_identity("some-agent-id"),
            agent_description(&[("agent_version", "1.0.0")], &[]),
        ),
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::default(),
        },
        Status {
            agent_control: fixed_agent_control(),
            fleet: fixed_fleet(),
            agents: SubAgentsStatus::from([(
                AgentID::try_from("some-agent-id").unwrap(),
                SubAgentStatus::with_identity(agent_identity("some-agent-id"))
                    .with_attributes(HashMap::from([(
                        "identifying/agent_version".to_string(),
                        "1.0.0".to_string(),
                    )])),
            )]),
        },
    )]
    #[tokio::test]
    async fn test_update_sub_agent_status(
        #[case] event: SubAgentEvent,
        #[case] current_status: Status,
        #[case] expected_status: Status,
    ) {
        let status = Arc::new(RwLock::new(current_status));
        update_sub_agent_status(event, status.clone()).await;
        assert_eq!(expected_status, *status.read().await);
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
