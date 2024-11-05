use super::config::{AuthConfig, LocalConfig, ProviderConfig};
use crate::opamp::auth::http_client::HttpClientUreq;
use crate::super_agent::run::BasePaths;
use chrono::DateTime;
use nr_auth::{
    authenticator::HttpAuthenticator,
    jwt::signer::{local::LocalPrivateKeySigner, JwtSignerImpl, JwtSignerImplError},
    token::{AccessToken, Token, TokenType},
    token_retriever::TokenRetrieverWithCache,
    TokenRetriever, TokenRetrieverError,
};
use std::time::Duration;
use thiserror::Error;
use tracing::error;

const DEFAULT_AUTHENTICATOR_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Error, Debug)]
pub enum TokenRetrieverImplError {
    #[error("building JWT signer: `{0}`")]
    JwtSignerBuildError(#[from] JwtSignerImplError),
}

// Just an alias to make the code more readable
type TokenRetrieverHttp = TokenRetrieverWithCache<HttpAuthenticator<HttpClientUreq>>;

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
        base_paths: BasePaths,
    ) -> Result<Self, TokenRetrieverImplError> {
        let Some(ac) = auth_config else {
            return Ok(Self::Noop(TokenRetrieverNoop));
        };

        Ok(Self::HttpTR(
            ac.try_into_token_retriever_with_cache(base_paths)?,
        ))
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

impl AuthConfig {
    pub fn try_into_token_retriever_with_cache(
        self,
        paths: BasePaths,
    ) -> Result<TokenRetrieverHttp, TokenRetrieverImplError> {
        let provider = self
            .provider
            .unwrap_or(ProviderConfig::Local(LocalConfig::new(
                paths.local_dir.clone(),
            )));

        let jwt_signer = JwtSignerImpl::try_from(provider)?;

        let http_client = HttpClientUreq::new(DEFAULT_AUTHENTICATOR_TIMEOUT);
        let authenticator = HttpAuthenticator::new(http_client, self.token_url.clone());

        Ok(
            TokenRetrieverWithCache::new(self.client_id, jwt_signer, authenticator)
                .with_retries(self.retries),
        )
    }
}

impl TryFrom<ProviderConfig> for JwtSignerImpl {
    type Error = JwtSignerImplError;
    fn try_from(value: ProviderConfig) -> Result<Self, Self::Error> {
        match value {
            ProviderConfig::Local(local_config) => Ok(JwtSignerImpl::Local(
                LocalPrivateKeySigner::try_from(local_config.private_key_path.as_path())?,
            )),
        }
    }
}
