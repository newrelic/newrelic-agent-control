use serde::Deserialize;

#[derive(Deserialize, Default, PartialEq, Debug, Clone)]
pub struct StatusCheckConfig {
    pub(super) endpoint: StatusEndpointConfig,
}

#[derive(Deserialize, Default, PartialEq, Debug, Clone)]
pub(super) struct StatusEndpointConfig {
    // Required
    pub(super) enabled: bool,

    #[serde(default)] // Pick the default port if the field is not included
    pub(super) port: Port,
    // Depending on what we can parameterize when implementing the endpoint itself,
    // we might want to add more fields here.
    // pub(super) timeout: u64,
    // pub(super) path: String, // should we hardcode it to "/status"?
}

#[derive(Deserialize, PartialEq, Debug, Clone)]
pub(super) struct Port(u16);

impl Default for Port {
    fn default() -> Self {
        Port(57475)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_status_check_config() {
        let config = r#"
            endpoint:
                enabled: true
                port: 8080
        "#;
        let deserialized: StatusCheckConfig = serde_yaml::from_str(config).unwrap();
        assert_eq!(
            deserialized,
            StatusCheckConfig {
                endpoint: StatusEndpointConfig {
                    enabled: true,
                    port: Port(8080)
                }
            }
        );
    }

    #[test]
    fn test_deserialize_status_check_config_missing_switch() {
        let config = r#"
            endpoint:
                port: 8080
        "#;
        let deserialized: Result<StatusCheckConfig, _> = serde_yaml::from_str(config);
        assert!(deserialized.is_err());
    }

    #[test]
    fn test_deserialize_status_check_config_default_port() {
        let config = r#"
            endpoint:
                enabled: true
        "#;
        let deserialized: StatusCheckConfig = serde_yaml::from_str(config).unwrap();
        assert_eq!(
            deserialized,
            StatusCheckConfig {
                endpoint: StatusEndpointConfig {
                    enabled: true,
                    port: Port(57475)
                }
            }
        );
    }
}
