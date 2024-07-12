use super::config::{AuthConfig, ProviderConfig};
use crate::super_agent::config::OpAMPClientConfig;
use chrono::DateTime;
use nr_auth::{
    authenticator::AuthenticatorConfig,
    jwt::signer::{local::LocalPrivateKeySigner, JwtSignerImpl, JwtSignerImplError},
    token::{AccessToken, Token, TokenType},
    token_retriever::TokenRetrieverWithCache,
    TokenRetriever, TokenRetrieverError,
};
use std::time::Duration;
use thiserror::Error;

const DEFAULT_AUTHENTICATOR_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Error, Debug)]
pub enum TokenRetrieverImplError {
    #[error("building JWT signer")]
    JwtSignerBuildError(#[from] JwtSignerImplError),
    #[error("provider not defined")]
    ProviderNotDefined,
}

/// Enumerates all implementations for `TokenRetriever` for static dispatching reasons.
#[allow(clippy::large_enum_variant)]
pub enum TokenRetrieverImpl {
    HttpTR(TokenRetrieverWithCache),
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

impl TryFrom<OpAMPClientConfig> for TokenRetrieverImpl {
    type Error = TokenRetrieverImplError;

    fn try_from(value: OpAMPClientConfig) -> Result<Self, Self::Error> {
        match value.auth_config {
            None => Ok(TokenRetrieverImpl::Noop(TokenRetrieverNoop)),
            Some(auth_config) => Ok(TokenRetrieverImpl::HttpTR(
                TokenRetrieverWithCache::try_from(auth_config)?,
            )),
        }
    }
}

/// Retrieves a default invalid token.
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

impl TryFrom<AuthConfig> for TokenRetrieverWithCache {
    type Error = TokenRetrieverImplError;

    fn try_from(config: AuthConfig) -> Result<Self, Self::Error> {
        let provider = config
            .provider
            .ok_or(TokenRetrieverImplError::ProviderNotDefined)?;

        let jwt_signer = JwtSignerImpl::try_from(provider)?;

        let authenticator_config = AuthenticatorConfig {
            timeout: DEFAULT_AUTHENTICATOR_TIMEOUT,
            url: config.token_url.clone(),
        };

        Ok(TokenRetrieverWithCache::new(
            config.client_id,
            config.token_url,
            jwt_signer,
            authenticator_config.into(),
        )
        .with_retries(config.retries))
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
