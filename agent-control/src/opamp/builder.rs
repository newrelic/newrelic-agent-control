use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::run::RunError;
use crate::http::config::ProxyConfig;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::http::builder::OpAMPHttpClientBuilder;
use std::sync::Arc;
use tracing::{debug, error};

/// Returns the OpAMP client builder corresponding to the provided arguments.
pub fn opamp_client_builder(
    opamp_config: OpAMPClientConfig,
    proxy_config: ProxyConfig,
    private_key: String,
) -> Result<OpAMPHttpClientBuilder<TokenRetrieverImpl>, RunError> {
    debug!("OpAMP configuration found, creating an OpAMP client builder");

    let token_retriever = Arc::new(
        TokenRetrieverImpl::try_build(
            opamp_config.clone().auth_config,
            private_key,
            proxy_config.clone(),
        )
        .inspect_err(|err| error!("Could not build OpAMP's token retriever: {err}"))
        .map_err(RunError::from)?,
    );

    let http_builder = OpAMPHttpClientBuilder::new(opamp_config, proxy_config, token_retriever);

    Ok(http_builder)
}
