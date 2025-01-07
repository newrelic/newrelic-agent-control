use crate::common::opamp::FakeServer;
use newrelic_agent_control::{opamp::instance_id::InstanceID, values::yaml_config::YAMLConfig};
use std::error::Error;

pub fn check_latest_effective_config_is_expected(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
    expected_config: YAMLConfig,
) -> Result<(), Box<dyn Error>> {
    // When opamp asks to get the effective config from the callback
    let effective_config = opamp_server.get_effective_config(instance_id.clone());

    if let Some(effective_cfg) = effective_config {
        let body_string = String::from_utf8(
            effective_cfg.config_map.clone().unwrap().config_map[""]
                .body
                .clone(),
        )?;
        let cfg_body = YAMLConfig::try_from(body_string)?;
        if expected_config != cfg_body {
            return Err(format!(
                "Effective config not as expected, Expected: {:?}, Found: {:?}",
                expected_config, cfg_body,
            )
            .into());
        }
    } else {
        return Err("No effective config created".into());
    }

    Ok(())
}
