use crate::agent_control::config::AgentID;
use crate::event::channel::EventConsumer;
use crate::event::{AgentControlEvent, SubAgentEvent};
use crate::sub_agent::health::health_checker::Health;
use crate::utils::threads::spawn_named_thread;
use crossbeam::select;
use std::thread::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info, warn};

/// Spawn an OS thread that will act as a bridge between the Sync Events in
/// the Agent Control and the Async Events in the Status Http Server
pub fn run_async_sync_bridge(
    async_sa_publisher: UnboundedSender<AgentControlEvent>,
    async_suba_publisher: UnboundedSender<SubAgentEvent>,
    agent_control_consumer: EventConsumer<AgentControlEvent>,
    sub_agent_consumer: EventConsumer<SubAgentEvent>,
) -> JoinHandle<()> {
    // Stores the current healthy state for logging purposes.
    let mut is_healthy = false;

    spawn_named_thread("Async-Sync bridge", move || loop {
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
                if let Err(err) = suba_event_res {
                    debug!(
                        error_msg = %err,
                        "status server bridge channel closed"
                    );
                    return;
                };

                let sub_agent_event = suba_event_res.expect("Error already handled");
                match &sub_agent_event {
                    SubAgentEvent::SubAgentHealthInfo(agent_id, _, health) => {
                        log_health_info(agent_id, is_healthy, health.clone().into());
                        is_healthy = health.is_healthy();
                    }
                }
                let _ = async_suba_publisher.send(sub_agent_event).inspect_err(|err| {
                    error!(
                        error_msg = %err,
                        "cannot forward agent control event"
                    );
                });
            }
        }
    })
}

fn log_health_info(agent_id: &AgentID, was_healthy: bool, health: Health) {
    match health {
        // From unhealthy (or initial) to healthy
        Health::Healthy(_) => {
            if !was_healthy {
                info!(%agent_id, "Agent is healthy");
            }
        }
        // Every time health is unhealthy
        Health::Unhealthy(unhealthy) => {
            warn!(%agent_id, status=unhealthy.status(), last_error=unhealthy.last_error(), "agent is unhealthy");
        }
    }
}
