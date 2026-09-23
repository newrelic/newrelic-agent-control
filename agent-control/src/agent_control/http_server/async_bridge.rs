//! Bridges synchronous Agent Control events onto async channels consumed by the status server.

use crate::event::channel::EventConsumer;
use crate::event::{AgentControlEvent, SubAgentEvent};
use crate::utils::threads::spawn_named_thread;
use crossbeam::channel::never;
use crossbeam::select;
use std::thread::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error};

/// Spawn an OS thread that will act as a bridge between the Sync Events in
/// the Agent Control and the Async Events in the Status Http Server
pub fn run_async_sync_bridge(
    async_sa_publisher: UnboundedSender<AgentControlEvent>,
    async_suba_publisher: UnboundedSender<SubAgentEvent>,
    agent_control_consumer: EventConsumer<AgentControlEvent>,
    sub_agent_consumer: EventConsumer<SubAgentEvent>,
    stop_rx: EventConsumer<()>,
) -> JoinHandle<()> {
    spawn_named_thread("Async-Sync bridge", move || {
        let ac_never = never::<AgentControlEvent>();
        let suba_never = never::<SubAgentEvent>();
        let mut ac_rx = agent_control_consumer.as_ref();
        let mut suba_rx = sub_agent_consumer.as_ref();

        loop {
            select! {
                recv(ac_rx) -> sa_event_res => {
                    match sa_event_res {
                        Ok(agent_control_event) => {
                            let _ = async_sa_publisher.send(agent_control_event).inspect_err(|err| {
                                error!(
                                    error_msg = %err,
                                    "Cannot forward agent control event"
                                );
                            });
                        }
                        Err(_) => ac_rx = &ac_never,
                    }
                },
                recv(suba_rx) -> suba_event_res => {
                    match suba_event_res {
                        Ok(sub_agent_event) => {
                            let _ = async_suba_publisher.send(sub_agent_event).inspect_err(|err| {
                                error!(
                                    error_msg = %err,
                                    "Cannot forward agent control event"
                                );
                            });
                        }
                        Err(_) => suba_rx = &suba_never,
                    }
                },
                recv(stop_rx.as_ref()) -> _ => {
                    debug!("status server bridge stopping");
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::channel::pub_sub;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn test_bridge_forwards_events() {
        let (async_sa_publisher, mut async_sa_consumer) = unbounded_channel::<AgentControlEvent>();
        let (async_suba_publisher, _async_suba_consumer) = unbounded_channel::<SubAgentEvent>();

        let (ac_publisher, ac_consumer) = pub_sub::<AgentControlEvent>();
        let (_suba_publisher, suba_consumer) = pub_sub::<SubAgentEvent>();
        let (stop_publisher, stop_consumer) = pub_sub::<()>();

        let bridge = run_async_sync_bridge(
            async_sa_publisher,
            async_suba_publisher,
            ac_consumer,
            suba_consumer,
            stop_consumer,
        );

        ac_publisher
            .publish(AgentControlEvent::OpAMPConnected)
            .unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            assert_eq!(
                async_sa_consumer.recv().await,
                Some(AgentControlEvent::OpAMPConnected)
            );
        });

        stop_publisher.try_publish(()).unwrap();
        bridge.join().unwrap();
    }

    #[test]
    fn test_bridge_stays_alive_after_channel_close_until_stop_rx() {
        let (async_sa_publisher, _async_sa_consumer) = unbounded_channel::<AgentControlEvent>();
        let (async_suba_publisher, _async_suba_consumer) = unbounded_channel::<SubAgentEvent>();

        let (ac_publisher, ac_consumer) = pub_sub::<AgentControlEvent>();
        let (_suba_publisher, suba_consumer) = pub_sub::<SubAgentEvent>();
        let (stop_publisher, stop_consumer) = pub_sub::<()>();

        let bridge = run_async_sync_bridge(
            async_sa_publisher,
            async_suba_publisher,
            ac_consumer,
            suba_consumer,
            stop_consumer,
        );

        // Close the agent-control publisher channel.
        drop(ac_publisher);

        // Give the bridge time to process the disconnect and disable the arm.
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Bridge must still be running, channel close alone does not exit it.
        assert!(!bridge.is_finished());

        stop_publisher.try_publish(()).unwrap();
        bridge.join().unwrap();
    }
}
