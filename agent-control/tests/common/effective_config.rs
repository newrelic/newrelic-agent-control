use crate::common::opamp::FakeServer;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use std::error::Error;

pub fn check_latest_effective_config_is_expected(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
    expected_config: String,
) -> Result<(), Box<dyn Error>> {
    // When opamp asks to get the effective config from the callback
    let effective_config = opamp_server.get_effective_config(instance_id.clone());

    if let Some(effective_cfg) = effective_config {
        let cfg_body = effective_cfg.config_map.clone().unwrap().config_map[""]
            .body
            .to_vec();
        let cfg_body_str = String::from_utf8(cfg_body).unwrap();
        // Avoid ordering and whitespace issues when comparing
        let cfg_yaml: serde_yaml::Value = serde_yaml::from_str(&cfg_body_str).unwrap();
        let expected_yaml: serde_yaml::Value = serde_yaml::from_str(&expected_config).unwrap();
        if cfg_yaml != expected_yaml {
            return Err(format!(
                "Effective config not as expected, Expected: {expected_config:?}, Found: {cfg_body_str:?}",
            )
            .into());
        }
    } else {
        return Err(format!("No effective config received for {instance_id}").into());
    }

    Ok(())
}
