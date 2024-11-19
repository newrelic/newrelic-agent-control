use std::{collections::HashMap, thread::sleep, time::Duration};

use opamp_client::{
    capabilities,
    error::ConnectionError,
    http::{HttpClientUreq, HttpConfig},
    opamp::proto::{
        AgentCapabilities, ComponentHealth, EffectiveConfig, OpAmpConnectionSettings,
        ServerErrorResponse, ServerToAgentCommand,
    },
    operation::{
        callbacks::{Callbacks, MessageData},
        settings::{AgentDescription, StartSettings},
    },
    Client, NotStartedClient, StartedClient,
};
use tracing::info;

use thiserror::Error;

struct CallbacksMock;

#[derive(Error, Debug)]
pub(crate) enum CallbacksMockError {}

impl Callbacks for CallbacksMock {
    type Error = CallbacksMockError;
    fn on_error(&self, _err: ServerErrorResponse) {}
    fn on_connect(&self) {
        info!("On connect callback called!")
    }
    fn on_message(&self, _msg: MessageData) {}
    fn on_command(&self, _command: &ServerToAgentCommand) -> Result<(), Self::Error> {
        Ok(())
    }
    fn on_connect_failed(&self, _err: ConnectionError) {}
    fn on_opamp_connection_settings(
        &self,
        _settings: &OpAmpConnectionSettings,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn on_opamp_connection_settings_accepted(&self, _settings: &OpAmpConnectionSettings) {}
    fn get_effective_config(
        &self,
    ) -> Result<opamp_client::opamp::proto::EffectiveConfig, Self::Error> {
        Ok(EffectiveConfig::default())
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::metadata::LevelFilter::INFO.into())
                .with_env_var("LOG_LEVEL")
                .from_env_lossy(),
        )
        .init();

    let http_config = HttpConfig::new("https://127.0.0.1/v1/opamp")
        .unwrap()
        .with_headers(HashMap::from([(
            "super-key".to_string(),
            "wooooooooooooooooooooow".to_string(),
        )]))
        .unwrap()
        .with_gzip_compression(false)
        .with_timeout(Duration::from_secs(5));

    let http_client_reqwest = HttpClientUreq::new(http_config).unwrap();

    let not_started_client = opamp_client::http::NotStartedHttpClient::new(http_client_reqwest);

    let client = not_started_client
        .start(
            CallbacksMock,
            StartSettings {
                instance_id: "3Q38XWW0Q98GMAD3NHWZM2PZWZ".into(),
                capabilities: capabilities!(AgentCapabilities::ReportsStatus),
                agent_description: AgentDescription {
                    identifying_attributes: HashMap::from([
                        ("service.name".to_string(), "com.newrelic.meta_agent".into()),
                        ("service.namespace".to_string(), "newrelic".into()),
                        ("service.version".to_string(), "0.2.0".into()),
                    ]),
                    non_identifying_attributes: HashMap::from([
                        ("key".to_string(), "val".into()),
                        ("int".to_string(), 5.into()),
                        ("bool".to_string(), true.into()),
                    ]),
                },
            },
        )
        .unwrap();

    client
        .set_health(ComponentHealth {
            healthy: true,
            status_time_unix_nano: 1689942447,
            last_error: "".to_string(),
            ..Default::default()
        })
        .unwrap();

    println!("sleeping");
    sleep(Duration::from_secs(30));
    println!("sleeping");

    client
        .set_health(ComponentHealth {
            healthy: false,
            status_time_unix_nano: 1689942447,
            last_error: "wow! what an error".to_string(),
            ..Default::default()
        })
        .unwrap();

    client.stop().unwrap()
}
