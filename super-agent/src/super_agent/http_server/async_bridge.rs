use crate::event::channel::EventConsumer;
use crate::event::{SubAgentEvent, SuperAgentEvent};
use crossbeam::select;
use std::thread;
use std::thread::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error};

/// Spawn an OS thread that will act as a bridge between the Sync Events in
/// the Super Agent and the Async Events in the Status Http Server
pub fn run_async_sync_bridge(
    async_sa_publisher: UnboundedSender<SuperAgentEvent>,
    async_suba_publisher: UnboundedSender<SubAgentEvent>,
    super_agent_consumer: EventConsumer<SuperAgentEvent>,
    sub_agent_consumer: EventConsumer<SubAgentEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || loop {
        select! {
            recv(&super_agent_consumer.as_ref()) -> sa_event_res => {
                match sa_event_res {
                    Ok(super_agent_event) => {
                        let _ = async_sa_publisher.send(super_agent_event).inspect_err(|err| {
                            error!(
                                error_msg = %err,
                                "cannot forward super agent event"
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
                                    "cannot forward super agent event"
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
    })
}
