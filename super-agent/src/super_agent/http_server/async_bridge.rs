use crate::event::channel::EventConsumer;
use crate::event::SuperAgentEvent;
use std::thread;
use std::thread::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error};

// Spawn an OS thread that will act as a bridge between the Sync Events in
// the Super Agent and the Async Events in the Status Http Server
pub fn run_async_sync_bridge(
    async_publisher: UnboundedSender<SuperAgentEvent>,
    super_agent_consumer: EventConsumer<SuperAgentEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || loop {
        match super_agent_consumer.as_ref().recv() {
            Ok(super_agent_event) => {
                let _ = async_publisher.send(super_agent_event).inspect_err(|e| {
                    error!(
                        error_msg = e.to_string(),
                        "cannot forward super agent event"
                    );
                });
            }
            Err(e) => {
                debug!(
                    error_msg = e.to_string(),
                    "status server bridge channel closed"
                );
                break;
            }
        }
    })
}

#[cfg(test)]
mod test {
    use crate::event::channel::pub_sub;
    use crate::event::SuperAgentEvent;
    use crate::event::SuperAgentEvent::{SubAgentBecameHealthy, SuperAgentBecameHealthy};
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::super_agent::http_server::async_bridge::run_async_sync_bridge;
    use std::thread;
    use std::thread::JoinHandle;
    use tokio::sync::mpsc;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_all_events_are_published() {
        let (tx, mut rx) = mpsc::unbounded_channel::<SuperAgentEvent>();
        let (super_agent_publisher, super_agent_consumer) = pub_sub::<SuperAgentEvent>();

        let join_handle = run_async_sync_bridge(tx, super_agent_consumer);

        let mut join_handles: Vec<JoinHandle<()>> = Vec::new();

        let super_agent_publisher_clone = super_agent_publisher.clone();
        join_handles.push(thread::spawn(move || {
            for _ in 0..5 {
                super_agent_publisher_clone
                    .publish(SuperAgentBecameHealthy)
                    .unwrap();
            }
        }));

        join_handles.push(thread::spawn(move || {
            for _ in 0..3 {
                super_agent_publisher
                    .publish(SubAgentBecameHealthy(
                        AgentID::new("some-agent-id").unwrap(),
                        AgentTypeFQN::from("whatever"),
                    ))
                    .unwrap();
            }
        }));

        while let Some(join_handle) = join_handles.pop() {
            join_handle.join().unwrap();
        }

        let mut total_events = 0;
        while rx.recv().await.is_some() {
            total_events += 1;
        }

        join_handle.join().unwrap();
        assert_eq!(8, total_events);
    }
}
