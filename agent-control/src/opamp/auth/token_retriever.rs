use super::config::{AuthConfig, LocalConfig, ProviderConfig};
use crate::http::client::HttpBuildError;
use crate::http::client::HttpClient;
use crate::http::config::HttpConfig;
use crate::http::config::ProxyConfig;
use chrono::DateTime;
use nr_auth::{
    TokenRetriever, TokenRetrieverError,
    authenticator::HttpAuthenticator,
    jwt::signer::{JwtSignerImpl, JwtSignerImplError, local::LocalPrivateKeySigner},
    token::{AccessToken, Token, TokenType},
    token_retriever::{TokenRetrieverWithCache, credential::JwtSignerAuthBuilder},
};
use std::time::Duration;
use thiserror::Error;

const DEFAULT_AUTHENTICATOR_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Error, Debug)]
pub enum TokenRetrieverImplError {
    #[error("building JWT signer: {0}")]
    JwtSignerBuildError(#[from] JwtSignerImplError),

    #[error("error building http client: {0}")]
    HTTPBuildingClientError(String),

    #[error("configuration error: {0}")]
    ConfigurationError(String),
}

// Just an alias to make the code more readable
type TokenRetrieverHttp =
    TokenRetrieverWithCache<HttpAuthenticator<HttpClient>, JwtSignerAuthBuilder<JwtSignerImpl>>;

/// Enumerates all implementations for `TokenRetriever` for static dispatching reasons.
#[allow(clippy::large_enum_variant)]
pub enum TokenRetrieverImpl {
    HttpTR(TokenRetrieverHttp),
    Noop(TokenRetrieverNoop),
}

impl TokenRetriever for TokenRetrieverImpl {
    fn retrieve(&self) -> Result<Token, TokenRetrieverError> {
        match self {
            TokenRetrieverImpl::HttpTR(token_retriever_with_cache) => {
                token_retriever_with_cache.retrieve()
            }
            TokenRetrieverImpl::Noop(noop_token_retriever) => noop_token_retriever.retrieve(),
        }
    }
}

impl TokenRetrieverImpl {
    pub fn try_build(
        auth_config: Option<AuthConfig>,
        private_key: Option<String>,
        proxy_config: ProxyConfig,
    ) -> Result<Self, TokenRetrieverImplError> {
        let Some(ac) = auth_config else {
            return Ok(Self::Noop(TokenRetrieverNoop));
        };

        let key = private_key.ok_or_else(|| {
            TokenRetrieverImplError::ConfigurationError(
                "Cannot load key: neither provider config or private string provided".to_string(),
            )
        })?;
        let provider = ProviderConfig::Local(LocalConfig::new_with_value(key));

        let jwt_signer = JwtSignerImpl::try_from(provider)?;

        let http_config = HttpConfig::new(
            DEFAULT_AUTHENTICATOR_TIMEOUT,
            DEFAULT_AUTHENTICATOR_TIMEOUT,
            proxy_config,
        );

        let client = HttpClient::new(http_config)?;
        let authenticator = HttpAuthenticator::new(client, ac.token_url.clone());

        Ok(Self::HttpTR(
            TokenRetrieverHttp::new_with_jwt_signer(ac.client_id, authenticator, jwt_signer)
                .with_retries(ac.retries),
        ))
    }
}

impl From<HttpBuildError> for TokenRetrieverImplError {
    fn from(err: HttpBuildError) -> Self {
        Self::HTTPBuildingClientError(err.to_string())
    }
}

/// Retrieves a default invalid token.
///
/// In the future the auth config an a TokenReceiver will be required
/// since there will be no more apy-key authentication.
/// This is a meantime solution to generate a TokenReceiver with no-operation and
/// avoid a bigger refactor in the future.
#[derive(Default)]
pub struct TokenRetrieverNoop;

impl TokenRetriever for TokenRetrieverNoop {
    fn retrieve(&self) -> Result<Token, TokenRetrieverError> {
        Ok(Token::new(
            AccessToken::default(),
            TokenType::Bearer,
            DateTime::default(),
        ))
    }
}

impl TryFrom<ProviderConfig> for JwtSignerImpl {
    type Error = JwtSignerImplError;

    fn try_from(value: ProviderConfig) -> Result<Self, Self::Error> {
        match value {
            ProviderConfig::Local(local_config) => {
                if let Some(key_content) = local_config.private_key_value {
                    let sanitized_key = key_content.replace("\\n", "\n");

                    let signer = LocalPrivateKeySigner::try_from(sanitized_key.as_bytes())?;
                    return Ok(JwtSignerImpl::Local(signer));
                }

                let signer =
                    LocalPrivateKeySigner::try_from(local_config.private_key_path.as_path())?;
                Ok(JwtSignerImpl::Local(signer))
            }
        }
    }
}
