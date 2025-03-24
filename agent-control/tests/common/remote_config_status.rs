use crate::common::opamp::FakeServer;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use std::error::Error;

pub fn check_latest_remote_config_status_is_expected(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
    expected_config_status: i32,
) -> Result<(), Box<dyn Error>> {
    // When opamp asks to get the effective config from the callback
    let remote_config_status = opamp_server.get_remote_config_status(instance_id.clone());

    if let Some(config_status) = remote_config_status.clone() {
        if expected_config_status != config_status.status {
            return Err(format!(
                "Remote config status not as expected, Expected: {:?}, Found: {:?}",
                expected_config_status, config_status.status,
            )
            .into());
        }
    } else {
        return Err("No Remote config statuses created".into());
    }

    Ok(())
}
