use crate::common::opamp::FakeServer;
use newrelic_super_agent::opamp::instance_id::InstanceID;
use std::error::Error;

pub fn check_latest_effective_config_is_expected(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
    expected_config: String,
) -> Result<(), Box<dyn Error>> {
    // When opamp asks to get the effective config from the callback
    let effective_config = opamp_server.get_effective_config(instance_id.clone());

    if let Some(effective_cfg) = effective_config.clone() {
        let cfg_body = effective_cfg.config_map.clone().unwrap().config_map[""]
            .body
            .to_vec();
        if expected_config.as_bytes() != cfg_body {
            return Err(format!(
                "Super agent config not as expected, Expected: {:?}, Found: {:?}",
                expected_config,
                String::from_utf8(cfg_body).unwrap(),
            )
            .into());
        }
    } else {
        return Err("No effective config created".into());
    }

    Ok(())
}
