#[cfg_attr(test, mockall_double::double)]
use crate::authenticator::HttpAuthenticator;
use crate::authenticator::{Authenticator, ClientAssertionType, GrantType, Request};
use crate::jwt::claims::Claims;
use crate::jwt::signer::JwtSigner;
#[cfg_attr(test, mockall_double::double)]
use crate::jwt::signer::JwtSignerImpl;
use crate::token::{Token, TokenType};
use crate::{ClientID, TokenRetriever, TokenRetrieverError};
use chrono::{TimeDelta, Utc};
use std::sync::Mutex;
use tracing::debug;
use url::Url;

/// A signed JWT should live enough for the System Identity Service to consume it.
const DEFAULT_JWT_CLAIM_EXP: TimeDelta = TimeDelta::seconds(180);

pub struct TokenRetrieverWithCache {
    client_id: ClientID,
    token_url: Url,
    tokens: Mutex<Option<Token>>,
    jwt_signer: JwtSignerImpl,
    authenticator: HttpAuthenticator,
    retries: u8,
}

impl TokenRetriever for TokenRetrieverWithCache {
    fn retrieve(&self) -> Result<Token, TokenRetrieverError> {
        let mut cached_token = self
            .tokens
            .lock()
            .map_err(|_| TokenRetrieverError::PoisonError)?;

        if cached_token.is_none() || cached_token.as_ref().is_some_and(|t| t.is_expired()) {
            // Attempt to refresh the token. Retry if failed.
            // This retry will block everyone trying to retrieve the token,
            // so we should enforce low retry numbers and error early.
            for i in 0..self.retries + 1 {
                match self.refresh_token() {
                    // Early exit if refreshed correctly.
                    Ok(token) => {
                        *cached_token = Some(token);
                        break;
                    }
                    // On error, log and retry if possible.
                    Err(e) => {
                        let retries_left = self.retries - i;
                        debug!("error refreshing token: {e}. Retries left {retries_left}");
                        if i < self.retries {
                            debug!("retrying to refresh token");
                        } else {
                            // If exhausted retries, return the err.
                            debug!("exhausted retries. Erroring out.");
                            return Err(e);
                        }
                    }
                }
            }
        }

        cached_token
            .to_owned()
            .ok_or(TokenRetrieverError::TokenRetrieverError(
                "getting token from cache".into(),
            ))
    }
}

impl TokenRetrieverWithCache {
    pub fn new(
        client_id: ClientID,
        token_url: Url,
        jwt_signer: JwtSignerImpl,
        authenticator: HttpAuthenticator,
    ) -> TokenRetrieverWithCache {
        TokenRetrieverWithCache {
            client_id,
            token_url,
            tokens: Mutex::new(None),
            jwt_signer,
            authenticator,
            retries: 0,
        }
    }

    pub fn with_retries(self, retries: u8) -> Self {
        Self { retries, ..self }
    }

    fn refresh_token(&self) -> Result<Token, TokenRetrieverError> {
        let expires_at = Utc::now() + DEFAULT_JWT_CLAIM_EXP;

        let timestamp = expires_at.timestamp().try_into().map_err(|_| {
            TokenRetrieverError::TokenRetrieverError("converting token expiration time".into())
        })?;

        let claims = Claims::new(
            self.client_id.to_owned(),
            self.token_url.to_owned(),
            timestamp,
        );

        let signed_jwt = self.jwt_signer.sign(claims)?;

        let request = Request {
            client_id: self.client_id.to_owned(),
            grant_type: GrantType::ClientCredentials,
            client_assertion_type: ClientAssertionType::JwtBearer,
            client_assertion: signed_jwt.value().into(),
        };

        let response = self.authenticator.authenticate(request)?;

        Ok(Token::new(
            response.access_token,
            TokenType::Bearer,
            Utc::now() + TimeDelta::seconds(response.expires_in.into()),
        ))
    }
}

#[cfg(test)]
mod test {
    use std::{thread, time};

    use chrono::{TimeDelta, Utc};
    use mockall::{predicate::eq, Sequence};
    use url::Url;

    #[cfg_attr(test, mockall_double::double)]
    use crate::authenticator::HttpAuthenticator;

    use crate::{
        authenticator::{AuthenticateError, ClientAssertionType, GrantType, Request, Response},
        jwt::signed::SignedJwt,
        token::{Token, TokenType},
        token_retriever::DEFAULT_JWT_CLAIM_EXP,
        TokenRetriever,
    };

    #[cfg_attr(test, mockall_double::double)]
    use crate::jwt::signer::JwtSignerImpl;

    use super::TokenRetrieverWithCache;

    #[test]
    // Test that a new token is retrieved when there is no cache and a cached token is
    // returned in case is not expired.
    fn retrieve_token_miss_hit_cache() {
        let token_url = "https://fake.com/";
        let client_id = "client_id";
        let token_expires_in = 1;

        let fake_client_assertion = "client_assertion";
        let fake_token = "fakeToken";

        let mut jwt_signer = JwtSignerImpl::new();
        jwt_signer
            .expect_sign()
            .once()
            .withf(move |claims| {
                let exp = Utc::now() + DEFAULT_JWT_CLAIM_EXP;
                claims.iss == client_id
                    && claims.aud == token_url
                    && claims.exp == exp.timestamp() as u64
            })
            .returning(move |_| {
                Ok(SignedJwt {
                    value: fake_client_assertion.into(),
                })
            });

        let expected_request = Request {
            client_id: client_id.to_owned(),
            grant_type: GrantType::ClientCredentials,
            client_assertion_type: ClientAssertionType::JwtBearer,
            client_assertion: fake_client_assertion.into(),
        };

        let mut authenticator = HttpAuthenticator::new();
        authenticator
            .expect_authenticate()
            .once()
            .with(eq(expected_request))
            .returning(move |_| {
                Ok(Response {
                    access_token: fake_token.into(),
                    expires_in: token_expires_in,
                    token_type: "".into(),
                })
            });

        let token_retriever = TokenRetrieverWithCache::new(
            client_id.into(),
            Url::parse(token_url).unwrap(),
            jwt_signer,
            authenticator,
        );

        let expected_token = Token::new(
            fake_token.into(),
            TokenType::Bearer,
            Utc::now() + TimeDelta::seconds(token_expires_in.into()),
        );

        let cache_miss_token = token_retriever.retrieve().unwrap();

        assert_eq!(
            expected_token.access_token(),
            cache_miss_token.access_token()
        );
        assert!(!cache_miss_token.is_expired());

        let cache_hit_token = token_retriever.retrieve().unwrap();

        assert_eq!(cache_miss_token, cache_hit_token);
    }

    #[test]
    // Test that a new token is retrieved when the cached one expired.
    fn retrieve_token_expired_cache() {
        let client_id = "client_id";
        let token_expires_in = 2;

        let mut jwt_signer = JwtSignerImpl::new();
        jwt_signer.expect_sign().times(2).returning(move |_| {
            Ok(SignedJwt {
                value: "client_assertion".into(),
            })
        });

        let mut authenticator = HttpAuthenticator::new();
        authenticator
            .expect_authenticate()
            .times(2)
            .returning(move |_| {
                Ok(Response {
                    // generates a different token each time.
                    access_token: Utc::now().to_string(),
                    expires_in: token_expires_in,
                    token_type: "bearer".into(),
                })
            });

        let token_retriever = TokenRetrieverWithCache::new(
            client_id.into(),
            Url::parse("https://fake.com/").unwrap(),
            jwt_signer,
            authenticator,
        );

        let cache_miss_token = token_retriever.retrieve().unwrap();

        // waits until the cached token expired + buffer to avoid flaky failures.
        thread::sleep(
            time::Duration::from_secs(token_expires_in.into()) + time::Duration::from_secs(1),
        );

        let cache_expired_token = token_retriever.retrieve().unwrap();

        assert_ne!(
            cache_expired_token.access_token(),
            cache_miss_token.access_token()
        )
    }

    #[test]
    fn retries_success() {
        let client_id = "client_id";
        let fake_token = "fake";
        let token_expires_in = 5;

        let mut jwt_signer = JwtSignerImpl::new();
        // This actually tests TokenRetrieverWithCache's `refresh_token`. Calling `retrieve` makes calls to both first `sign` and then to `authenticate`. We instruct the `authenticate` call to fail every time and check that `sign` is called only the number of times we expect.
        jwt_signer.expect_sign().times(2).returning(move |_| {
            Ok(SignedJwt {
                value: "client_assertion".into(),
            })
        });

        let mut auth_sequence = Sequence::new();
        let mut authenticator = HttpAuthenticator::new();
        authenticator
            .expect_authenticate()
            .once()
            .in_sequence(&mut auth_sequence)
            .returning(move |_| {
                Err(AuthenticateError::DeserializeError(
                    "some_serde_error".to_owned(),
                ))
            });

        authenticator
            .expect_authenticate()
            .once()
            .returning(move |_| {
                Ok(Response {
                    // generates a different token each time.
                    access_token: fake_token.into(),
                    expires_in: token_expires_in,
                    token_type: "bearer".into(),
                })
            });

        let token_retriever = TokenRetrieverWithCache::new(
            client_id.into(),
            Url::parse("https://fake.com/").unwrap(),
            jwt_signer,
            authenticator,
        )
        .with_retries(2);

        let expected_token = Token::new(
            fake_token.into(),
            TokenType::Bearer,
            Utc::now() + TimeDelta::seconds(token_expires_in.into()),
        );

        // Retries expired, error returned
        let cache_miss_token = token_retriever.retrieve().unwrap();

        assert_eq!(
            cache_miss_token.access_token(),
            expected_token.access_token()
        );
    }

    #[test]
    fn retries_fail() {
        let client_id = "client_id";

        let mut jwt_signer = JwtSignerImpl::new();
        // This actually tests TokenRetrieverWithCache's `refresh_token`. Calling `retrieve` makes calls to both first `sign` and then to `authenticate`. We instruct the `authenticate` call to fail every time and check that `sign` is called only the number of times we expect.
        jwt_signer.expect_sign().times(3).returning(move |_| {
            Ok(SignedJwt {
                value: "client_assertion".into(),
            })
        });

        let mut authenticator = HttpAuthenticator::new();
        authenticator.expect_authenticate().returning(move |_| {
            Err(AuthenticateError::DeserializeError(
                "some_serde_error".to_owned(),
            ))
        });

        let token_retriever = TokenRetrieverWithCache::new(
            client_id.into(),
            Url::parse("https://fake.com/").unwrap(),
            jwt_signer,
            authenticator,
        )
        .with_retries(2);

        // Retries expired, error returned
        let cache_miss_token = token_retriever.retrieve();

        assert!(cache_miss_token.is_err());
    }
}
