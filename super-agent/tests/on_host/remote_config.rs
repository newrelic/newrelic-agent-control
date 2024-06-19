use crate::common::opamp::{ConfigResponse, FakeServer};
use crate::common::retry::retry;
use http::HeaderMap;
use newrelic_super_agent::event::channel::pub_sub;
use newrelic_super_agent::event::SuperAgentEvent;
use newrelic_super_agent::opamp::auth::token_retriever::{TokenRetrieverImpl, TokenRetrieverNoop};
use newrelic_super_agent::opamp::client_builder::DefaultOpAMPClientBuilder;
use newrelic_super_agent::opamp::http::builder::UreqHttpClientBuilder;
use newrelic_super_agent::opamp::instance_id::getter::{
    InstanceIDGetter, InstanceIDWithIdentifiersGetter,
};
use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
use newrelic_super_agent::super_agent::config::{
    AgentID, OpAMPClientConfig, SuperAgentDynamicConfig,
};
use newrelic_super_agent::super_agent::config_storer::store::SuperAgentConfigStore;
use newrelic_super_agent::super_agent::defaults::{set_local_dir, set_remote_dir};
use newrelic_super_agent::super_agent::run::run_super_agent;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use url::Url;

#[cfg(unix)]
#[test]
fn onhost_opamp_superagent_configuration_change() {
    // The local configuration for the open-telemetry collector is valied, then the remote configuration
    // is loaded and applied.

    let mut server = FakeServer::start_new();
    let server_endpoint = Url::try_from(server.endpoint().as_str()).unwrap();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    set_local_dir(local_dir.path());
    set_remote_dir(remote_dir.path());

    let config_file_path = local_dir.path().join("config.yaml");
    let mut local_file =
        File::create(config_file_path.clone()).expect("failed to create local config file");
    let local_config = r#"
host_id: integration-test
fleet_id: integration
opamp:
  endpoint: http://127.0.0.1/v1/opamp
agents: {}
"#;
    write!(local_file, "{}", local_config).unwrap();

    // We won't join and wait for the thread to finish because we want the super_agent to exit
    // if our assertions were not ok.
    let _super_agent_join = thread::spawn(move || {
        // Create the Tokio runtime
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        );

        let token_retriever = Arc::new(TokenRetrieverImpl::Noop(TokenRetrieverNoop {}));

        let opamp_config = OpAMPClientConfig {
            endpoint: server_endpoint,
            headers: HeaderMap::default(),
            auth_config: None,
        };

        let http_builder = UreqHttpClientBuilder::new(opamp_config.clone(), token_retriever);
        let builder = Some(DefaultOpAMPClientBuilder::new(
            opamp_config.clone(),
            http_builder,
        ));

        let (_application_event_publisher, application_event_consumer) = pub_sub();
        let (super_agent_publisher, _super_agent_consumer) = pub_sub::<SuperAgentEvent>();

        let config_storer = SuperAgentConfigStore::new(config_file_path.as_path());

        _ = run_super_agent(
            runtime.clone(),
            config_storer,
            application_event_consumer,
            builder,
            super_agent_publisher,
        );
    });

    let super_agent_id = &AgentID::new_super_agent_id();

    let identifiers_provider = IdentifiersProvider::default()
        .with_host_id("integration-test".to_string())
        .with_fleet_id("integration".to_string());
    let identifiers = identifiers_provider.provide().unwrap_or_default();

    let instance_id_getter =
        InstanceIDWithIdentifiersGetter::default().with_identifiers(identifiers);
    let super_agent_instance_id = instance_id_getter.get(super_agent_id);

    // Update the agent configuration via OpAMP
    server.set_config_response(
        super_agent_instance_id.unwrap(),
        ConfigResponse::from(
            r#"
agents:
  nr-infra-agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.1.2"
  otel-collector:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
        ),
    );

    // Check the expected HelmRelease is created with the spec values
    let expected_config = r#"agents:
  nr-infra-agent:
    agent_type: newrelic/com.newrelic.infrastructure_agent:0.1.2
  otel-collector:
    agent_type: newrelic/io.opentelemetry.collector:0.0.1
"#;
    let expected_config_parsed =
        serde_yaml::from_str::<SuperAgentDynamicConfig>(expected_config).unwrap();

    retry(20, Duration::from_secs(5), || {
        || -> Result<(), Box<dyn Error>> {
            let remote_file = remote_dir.path().join("config.yaml");
            let content =
                std::fs::read_to_string(remote_file.as_path()).unwrap_or("agents:".to_string());
            let content_parsed =
                serde_yaml::from_str::<SuperAgentDynamicConfig>(content.as_str()).unwrap();
            if content_parsed != expected_config_parsed {
                return Err(format!(
                    "Super agent config not as expected, Expected: {:?}, Found: {:?}",
                    expected_config, content,
                )
                .into());
            }
            Ok(())
        }()
    });
}
