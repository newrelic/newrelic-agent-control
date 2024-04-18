use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::event::SuperAgentEvent;
use crate::super_agent::http_server::status::{Status, SubAgentStatus};

pub(super) async fn on_super_agent_event_update_status(
    mut sa_event_consumer: UnboundedReceiver<SuperAgentEvent>,
    status: Arc<RwLock<Status>>,
) {
    while let Some(super_agent_event) = sa_event_consumer.recv().await {
        match super_agent_event {
            SuperAgentEvent::SuperAgentBecameHealthy => {
                debug!("status_http_server event_processor super_agent_became_healthy");
                let mut status = status.write().await;
                status.super_agent.healthy = true;
                status.super_agent.last_error = String::default();
            }
            SuperAgentEvent::SuperAgentBecameUnhealthy(error_msg) => {
                debug!(
                    error_msg,
                    "status_http_server event_processor super_agent_became_unhealthy"
                );
                let mut status = status.write().await;
                status.super_agent.healthy = false;
                status.super_agent.last_error = error_msg;
            }
            SuperAgentEvent::SubAgentBecameUnhealthy(agent_id, agent_type, error_msg) => {
                debug!(error_msg, %agent_id, %agent_type, "status_http_server event_processor sub_agent_became_unhealthy");
                let mut status = status.write().await;
                status
                    .sub_agents
                    .entry(agent_id.clone())
                    .or_insert_with(|| SubAgentStatus::new(agent_id, agent_type))
                    .unhealthy(error_msg);
            }
            SuperAgentEvent::SubAgentBecameHealthy(agent_id, agent_type) => {
                debug!(%agent_id, %agent_type, "status_http_server event_processor sub_agent_became_healthy");
                let mut status = status.write().await;
                status
                    .sub_agents
                    .entry(agent_id.clone())
                    .or_insert_with(|| SubAgentStatus::new(agent_id, agent_type))
                    .healthy();
            }
            SuperAgentEvent::SubAgentRemoved(agent_id) => {
                let mut status = status.write().await;
                status.sub_agents.remove(&agent_id);
            }
            SuperAgentEvent::SuperAgentStopped => {
                debug!("status http server super agent stopped event");
                break;
            }
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
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::super_agent::http_server::status::{Status, SubAgentStatus};
    use crate::super_agent::http_server::status_updater::on_super_agent_event_update_status;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_super_agent_gets_healthy() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<SuperAgentEvent>();
        // Given an unhealthy Super Agent
        let status = Arc::new(RwLock::new(
            Status::default().with_unhealthy_super_agent(String::from("last error")),
        ));

        // When an event for Super Agent is received
        let publisher_clone = sa_event_publisher.clone();
        let publisher_handle = rt.spawn(async move {
            publisher_clone.send(SuperAgentBecameHealthy).unwrap();

            sleep(Duration::from_millis(10)).await;
            publisher_clone.send(SuperAgentStopped).unwrap();
        });

        // Then the event will be consumed
        on_super_agent_event_update_status(sa_event_consumer, status.clone()).await;
        publisher_handle.await.unwrap();
        // And status will contain the Sub Agent information
        let st = status.read().await;
        assert!(st.super_agent.healthy);
        assert_eq!(String::default(), st.super_agent.last_error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_super_agent_gets_unhealthy() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<SuperAgentEvent>();
        // Given an unhealthy Super Agent
        let status = Arc::new(RwLock::new(Status::default().with_healthy_super_agent()));

        // When an event for Super Agent is received
        let publisher_clone = sa_event_publisher.clone();
        let publisher_handle = rt.spawn(async move {
            publisher_clone
                .send(SuperAgentBecameUnhealthy(String::from("unhealthy error")))
                .unwrap();

            sleep(Duration::from_millis(10)).await;
            publisher_clone.send(SuperAgentStopped).unwrap();
        });

        // Then the event will be consumed
        on_super_agent_event_update_status(sa_event_consumer, status.clone()).await;
        publisher_handle.await.unwrap();
        // And status will contain the Sub Agent information
        let st = status.read().await;
        assert!(!st.super_agent.healthy);
        assert_eq!(String::from("unhealthy error"), st.super_agent.last_error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_sub_agent_first_event() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<SuperAgentEvent>();
        // Given there are no sub agents registered yet
        let status = Arc::new(RwLock::new(Status::default()));

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentTypeFQN::from("some-agent-type");

        // (spoiler: expected sub agent)
        let mut expected = SubAgentStatus::new(agent_id.clone(), agent_type.clone());
        expected.healthy();

        // When an event for a Sub Agent is received
        let publisher_clone = sa_event_publisher.clone();
        let publisher_handle = rt.spawn(async move {
            publisher_clone
                .send(SubAgentBecameHealthy(agent_id, agent_type))
                .unwrap();

            sleep(Duration::from_millis(10)).await;
            publisher_clone.send(SuperAgentStopped).unwrap();
        });

        // Then the event will be consumed
        on_super_agent_event_update_status(sa_event_consumer, status.clone()).await;
        publisher_handle.await.unwrap();
        // And status will contain the Sub Agent information
        let st = status.read().await;
        assert_eq!(1, st.sub_agents.as_collection().len());
        assert_eq!(&expected, st.sub_agents.as_collection().first().unwrap());
    }

    #[tokio::test]
    async fn test_sub_agent_became_unhealthy() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<SuperAgentEvent>();
        // Given there is a healthy Sub Agent registered
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentTypeFQN::from("some-agent-type");
        let mut sub_agent_status = SubAgentStatus::new(agent_id.clone(), agent_type.clone());
        sub_agent_status.healthy();

        let sub_agents = HashMap::from([(
            agent_id.clone(),
            SubAgentStatus::new(agent_id.clone(), agent_type.clone()),
        )]);

        let status = Arc::new(RwLock::new(
            Status::default().with_sub_agents(sub_agents.into()),
        ));

        // (spoiler: expected sub agent)
        let last_error = String::from("this is a horrible error");
        let mut expected = SubAgentStatus::new(agent_id.clone(), agent_type.clone());
        expected.unhealthy(last_error.clone());

        // When an event for a Sub Agent is received
        let publisher_clone = sa_event_publisher.clone();
        let agent_id_clone = agent_id.clone();
        let publisher_handle = rt.spawn(async move {
            publisher_clone
                .send(SubAgentBecameUnhealthy(
                    agent_id_clone,
                    agent_type,
                    last_error,
                ))
                .unwrap();

            sleep(Duration::from_millis(10)).await;
            publisher_clone.send(SuperAgentStopped).unwrap();
        });

        // Then the event will be consumed
        on_super_agent_event_update_status(sa_event_consumer, status.clone()).await;
        publisher_handle.await.unwrap();
        // And status will contain the Sub Agent information
        let st = status.read().await;
        assert_eq!(1, st.sub_agents.as_collection().len());
        assert_eq!(&expected, st.sub_agents.get(&agent_id).unwrap());
    }

    #[tokio::test]
    async fn test_sub_agent_removed() {
        let rt = Handle::current();
        let (sa_event_publisher, sa_event_consumer) = unbounded_channel::<SuperAgentEvent>();
        // Given there are two a healthy Sub Agent registered
        let agent_id1 = AgentID::new("some-agent-id-1").unwrap();
        let agent_type1 = AgentTypeFQN::from("some-agent-type-1");
        let sub_agent_status1 = SubAgentStatus::new(agent_id1.clone(), agent_type1.clone());

        let agent_id2 = AgentID::new("some-agent-id-2").unwrap();
        let agent_type2 = AgentTypeFQN::from("some-agent-type-2");
        let sub_agent_status2 = SubAgentStatus::new(agent_id2.clone(), agent_type2.clone());

        let agent_id3 = AgentID::new("some-agent-id-3").unwrap();
        let agent_type3 = AgentTypeFQN::from("some-agent-type-3");
        let sub_agent_status3 = SubAgentStatus::new(agent_id3.clone(), agent_type3.clone());

        let sub_agents = HashMap::from([
            (agent_id1.clone(), sub_agent_status1),
            (agent_id2.clone(), sub_agent_status2),
            (agent_id3.clone(), sub_agent_status3),
        ]);

        let status = Arc::new(RwLock::new(
            Status::default().with_sub_agents(sub_agents.into()),
        ));

        // When an event for a Sub Agent is received
        let publisher_clone = sa_event_publisher.clone();
        let agent_id_clone = agent_id2.clone();
        let publisher_handle = rt.spawn(async move {
            publisher_clone
                .send(SubAgentRemoved(agent_id_clone))
                .unwrap();

            sleep(Duration::from_millis(10)).await;
            publisher_clone.send(SuperAgentStopped).unwrap();
        });

        // Then the event will be consumed
        on_super_agent_event_update_status(sa_event_consumer, status.clone()).await;
        publisher_handle.await.unwrap();
        // And status will contain the Sub Agent information
        let st = status.read().await;
        assert_eq!(2, st.sub_agents.as_collection().len());
        assert!(st.sub_agents.get(&agent_id1).is_some());
        assert!(st.sub_agents.get(&agent_id3).is_some());
    }
}
