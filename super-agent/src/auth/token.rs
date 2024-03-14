use crate::auth::authenticator::{
    AuthenticateError, Authenticator, ClientAssertionType, GrantType,
};
use crate::auth::jwt::{JwtEncoderError, JwtSigner};
use crate::auth::token::TokenRetrieverError::PoisonError;
use crate::auth::{authenticator, AccessToken, ClientID};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum TokenRetrieverError {
    #[error("jwt encoder error: `{0}`")]
    JwtError(#[from] JwtEncoderError),
    #[error("auth error: `{0}`")]
    AuthError(#[from] AuthenticateError),
    #[error("poison error")]
    PoisonError,
}

#[derive(Clone, Debug)]
pub enum TokenType {
    Bearer,
}

/// Token structs represents the JWT returned by Oauth service containing:
/// {
///    "nr_identity_id": "1000000000",
///    "nr_orgid": "bcdef-abccd-12312-abbcdd",
///    "token_version": 1.0
/// }
/// It will used to make authorized requests to the backend
/// https://docs.google.com/document/d/1SM6mMqlhFMd2Tam-wp1kze0tgz5R9ZgepRffXMQWwp4/edit#heading=h.1tgvcer130l
/// example:
/// GET /protectedResource
///
/// Headers MUST include:
/// Authorization: Bearer <ACCESS TOKEN HERE>
#[derive(Clone, Debug)]
pub struct Token {
    expires_at: DateTime<Utc>,
    access_token: AccessToken,
    token_type: TokenType,
}

impl Token {
    fn new(access_token: AccessToken, token_type: TokenType, expires_at: DateTime<Utc>) -> Self {
        Token {
            access_token,
            token_type,
            expires_at,
        }
    }

    // pub just for test
    pub fn is_expired(&self) -> bool {
        self.expires_at.lt(&Utc::now())
    }

    pub fn access_token(&self) -> AccessToken {
        self.access_token.clone()
    }
}

pub trait TokenRetriever {
    fn retrieve(&self) -> Result<Token, TokenRetrieverError>;
}

pub struct TokenRetrieverWithCache<S, A> {
    client_id: ClientID,
    tokens: Arc<Mutex<Option<Token>>>,
    jwt_signer: S,
    authenticator: A,
    expires_in_seconds: u32,
}

impl<S, A> TokenRetriever for TokenRetrieverWithCache<S, A>
where
    S: JwtSigner,
    A: Authenticator,
{
    fn retrieve(&self) -> Result<Token, TokenRetrieverError> {
        let mut cached_token = self.tokens.lock().map_err(|_| PoisonError)?;

        // after checking the key existence cached_token.get(&client_id) is satisfied
        if cached_token.is_none() || cached_token.as_ref().unwrap().is_expired() {
            // TODO this is just for POC purpose
            if cached_token.is_none() {
                info!("Not token for client {}", self.client_id);
            } else if cached_token.as_ref().unwrap().is_expired() {
                info!("Token expired");
            }

            info!("Refreshing token");
            let token = self.refresh_token()?;

            *cached_token = Some(token);
        } else {
            info!(
                "not expired token. Token expires at: {}",
                cached_token.as_ref().unwrap().expires_at
            )
        }

        // at this point cached_token.as_ref().unwrap() is satisfied
        Ok(cached_token.as_ref().unwrap().clone())
    }
}

impl<S, A> TokenRetrieverWithCache<S, A>
where
    S: JwtSigner,
    A: Authenticator,
{
    pub fn new(
        client_id: ClientID,
        expires_in_seconds: u32,
        jwt_signer: S,
        authenticator: A,
    ) -> TokenRetrieverWithCache<S, A> {
        TokenRetrieverWithCache {
            client_id,
            tokens: Arc::new(Mutex::new(None)),
            expires_in_seconds,
            jwt_signer,
            authenticator,
        }
    }

    fn refresh_token(&self) -> Result<Token, TokenRetrieverError> {
        let expires_at = Utc::now() + Duration::seconds(self.expires_in_seconds as i64);
        let claims = crate::auth::jwt::Claims::new(
            self.client_id.clone(),
            crate::auth::jwt::FULL_URL_TO_OUR_TOKEN_GENERATION_ENDPOINT.to_owned(),
            crate::auth::jwt::jti(),
            expires_at.timestamp_millis() as usize,
        );

        let signed_jwt = self.jwt_signer.sign(claims)?;

        let request = authenticator::Request::new(
            &self.client_id,
            GrantType::ClientCredentials,
            ClientAssertionType::JwtBearer,
            signed_jwt.value().into(),
        );

        let response = self.authenticator.authenticate(request)?;

        Ok(Token::new(
            response.access_token().clone(),
            TokenType::Bearer,
            signed_jwt.expires_at(),
        ))
    }
}
