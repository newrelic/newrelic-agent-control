use http::HeaderMap;
use newrelic_super_agent::event::channel::pub_sub;
use newrelic_super_agent::event::SuperAgentEvent;
use newrelic_super_agent::opamp::auth::token_retriever::{TokenRetrieverImpl, TokenRetrieverNoop};
use newrelic_super_agent::opamp::client_builder::DefaultOpAMPClientBuilder;
use newrelic_super_agent::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use newrelic_super_agent::opamp::http::builder::UreqHttpClientBuilder;
use newrelic_super_agent::super_agent::config::OpAMPClientConfig;
use newrelic_super_agent::super_agent::config_storer::file::SuperAgentConfigStoreFile;
use newrelic_super_agent::super_agent::run::on_host::run_super_agent;
use std::path::Path;
use std::sync::Arc;
use url::Url;

/// Starts the super-agent through [start_super_agent] after setting up the corresponding configuration file
/// and config map according to the provided `folder_name` and the provided `file_names`.
pub fn start_super_agent_with_custom_config(config_path: &Path, opamp_endpoint: Url) {
    // Create the Tokio runtime
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap(),
    );

    let token_retriever = Arc::new(TokenRetrieverImpl::Noop(TokenRetrieverNoop {}));

    let opamp_config = OpAMPClientConfig {
        endpoint: opamp_endpoint,
        headers: HeaderMap::default(),
        auth_config: None,
    };

    let http_builder = UreqHttpClientBuilder::new(opamp_config.clone(), token_retriever);
    let effective_config_loader_builder = DefaultEffectiveConfigLoaderBuilder;

    let builder = Some(DefaultOpAMPClientBuilder::new(
        opamp_config.clone(),
        http_builder,
        effective_config_loader_builder,
    ));

    let (_application_event_publisher, application_event_consumer) = pub_sub();
    let (super_agent_publisher, _super_agent_consumer) = pub_sub::<SuperAgentEvent>();

    let config_storer = SuperAgentConfigStoreFile::new(config_path);

    _ = run_super_agent(
        runtime.clone(),
        config_storer,
        application_event_consumer,
        builder,
        super_agent_publisher,
    );
}
