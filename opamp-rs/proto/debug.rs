use std::fmt::Display;

trait Stringify {
    fn to_string(&self) -> String;
}

// Extension trait for Option type
impl<T> Stringify for Option<T>
where
    T: Display,
{
    fn to_string(&self) -> String {
        self.as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "None".to_string())
    }
}

// Extension trait for Option type
impl<T, E> Stringify for Result<T, E>
where
    T: Display,
    E: Display,
{
    fn to_string(&self) -> String {
        self.as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|e| e.to_string())
    }
}

// TODO: create a macro so we can auto implemented it for every Struct with a Vec<u8> attribute
impl Display for crate::opamp::proto::ServerToAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let remote_config = self.remote_config.to_string();
        let instance_uid: String = String::from_utf8_lossy(&self.instance_uid).to_string();
        write!(
            f,
            "ServerToAgent {{ instance_uid: \"{}\", error_response: {:?}, remote_config: {} }}",
            instance_uid, self.error_response, remote_config
        )
    }
}

impl Display for crate::opamp::proto::AgentRemoteConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AgentRemoteConfig {{ config: {}, config_hash: \"{}\" }}",
            self.config.to_string(),
            std::str::from_utf8(&self.config_hash).to_string()
        )
    }
}

impl Display for crate::opamp::proto::AgentConfigMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let config_map = self
            .config_map
            .iter()
            .map(|(key, value)| format!("\"{}\": {}", key, value))
            .collect::<Vec<_>>()
            .join(",");

        write!(f, "AgentConfigMap {{ config_map: {} }}", config_map)
    }
}

impl Display for crate::opamp::proto::AgentConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"AgentConfigFile {{ body: "{}", content_type: "{}" }}"#,
            std::str::from_utf8(&self.body).to_string(),
            self.content_type
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn assert_display() {
        let expected_string = r#"ServerToAgent { instance_uid: "01HF9B5C334ZJW3GA0HPGHDTDV", error_response: None, remote_config: AgentRemoteConfig { config: AgentConfigMap { config_map: "test-fleet-list-erich": AgentConfigFile { body: "hocus pocus", content_type: "text/yaml" } }, config_hash: "5298c05a10e7ca5c91edab6453bb60e8f2848491c15af0f2b2fe8710ad63c34f" } }"#;
        let sample_message = ServerToAgent {
            instance_uid: "01HF9B5C334ZJW3GA0HPGHDTDV".into(),
            remote_config: Some(AgentRemoteConfig {
                config: Some(AgentConfigMap {
                    config_map: std::collections::HashMap::from([(
                        "test-fleet-list-erich".to_string(),
                        AgentConfigFile {
                            body: "hocus pocus".into(),
                            content_type: "text/yaml".to_string(),
                        },
                    )]),
                }),
                config_hash: "5298c05a10e7ca5c91edab6453bb60e8f2848491c15af0f2b2fe8710ad63c34f"
                    .into(),
            }),
            ..Default::default()
        };

        eprintln!("{}", sample_message);

        assert!(sample_message.to_string() == expected_string);
    }
}
