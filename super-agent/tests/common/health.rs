use crate::common::opamp::FakeServer;
use newrelic_super_agent::opamp::instance_id::InstanceID;
use std::error::Error;

pub fn check_latest_health_status_was_healthy(
    server: &FakeServer,
    instance_id: &InstanceID,
) -> Result<(), Box<dyn Error>> {
    let health_status = server.get_health_status(instance_id.clone());
    match health_status {
        Some(status) if status.healthy => Ok(()),
        None => Err("Health status not available".into()),
        _ => Err("Expected healthy status, got unhealthy".into()),
    }
}
