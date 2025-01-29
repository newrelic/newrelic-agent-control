use crate::event::channel::EventConsumer;
use crate::event::{AgentControlEvent, SubAgentEvent};
use crossbeam::select;
use std::thread;
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
) -> JoinHandle<()> {
    thread::Builder::new().name("AC sync events to/from Status Http server async events bridge thread".to_string()).spawn(move || loop {
        select! {
            recv(&agent_control_consumer.as_ref()) -> sa_event_res => {
                match sa_event_res {
                    Ok(agent_control_event) => {
                        let _ = async_sa_publisher.send(agent_control_event).inspect_err(|err| {
                            error!(
                                error_msg = %err,
                                "cannot forward agent control event"
                            );
                        });
                    }
                    Err(err) => {
                        debug!(
                            error_msg = %err,
                            "status server bridge channel closed"
                        );
                        break;
                    }
                }
            },
            recv(&sub_agent_consumer.as_ref()) -> suba_event_res => {
                    match suba_event_res {
                        Ok(sub_agent_event) => {
                            let _ = async_suba_publisher.send(sub_agent_event).inspect_err(|err| {
                                error!(
                                    error_msg = %err,
                                    "cannot forward agent control event"
                                );
                            });
                        }
                        Err(err) => {
                            debug!(
                                error_msg = %err,
                                "status server bridge channel closed"
                            );
                            break;
                        }
                    }
                }
        }
    }).expect("thread config should be valid")
}
