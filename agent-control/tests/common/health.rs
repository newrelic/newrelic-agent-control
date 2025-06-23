use crate::common::opamp::FakeServer;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use opamp_client::opamp::proto::ComponentHealth;
use std::error::Error;

pub fn check_latest_health_status_was_healthy(
    server: &FakeServer,
    instance_id: &InstanceID,
) -> Result<(), Box<dyn Error>> {
    check_latest_health_status(server, instance_id, |status| status.healthy)
}

/// Returns Ok if the latest health receive for the provided `instance_id` in the OpAMP testing `server` match the
/// expectations set in `check_fn`.
pub fn check_latest_health_status(
    server: &FakeServer,
    instance_id: &InstanceID,
    check_fn: impl FnOnce(&ComponentHealth) -> bool,
) -> Result<(), Box<dyn Error>> {
    let health_status = server.get_health_status(instance_id);
    match health_status.as_ref() {
        Some(status) if check_fn(status) => Ok(()),
        None => Err("Health status not available".into()),
        _ => Err(format!(
            "The latest health status didn't match the expectations: {health_status:?}"
        )
        .into()),
    }
}
