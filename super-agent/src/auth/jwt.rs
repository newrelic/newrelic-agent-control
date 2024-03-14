use crate::auth::{ClientID, SignedJwtValue};
use chrono::{DateTime, TimeZone, Utc};
use jsonwebtoken::{encode, errors, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;
use ulid::Ulid;

pub(super) const FULL_URL_TO_OUR_TOKEN_GENERATION_ENDPOINT: &str =
    "https://staging-system-identity-poc.vip.cf.nr-ops.net";

type Jti = String; // JWT ID. Unique identifier

#[derive(Error, Debug)]
pub enum JwtEncoderError {
    #[error("unable to encode token: `{0}`")]
    EncodeTokenError(#[from] errors::Error),
}

pub trait JwtSigner {
    fn sign(&self, claims: Claims) -> Result<SignedJwt, JwtEncoderError>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    iss: String, // Issuer: ClientID
    sub: String, // Subject (whom token refers to): ClientID
    aud: String, // Audience: full URL to our token generation endpoint
    jti: String, // JWT ID. Unique identifier
    exp: usize,  // Expiration time (as UTC timestamp)
}

impl Claims {
    pub fn new(client_id: ClientID, url: String, jwt_id: String, exp: usize) -> Self {
        Self {
            iss: client_id.clone(),
            sub: client_id,
            aud: url,
            jti: jwt_id,
            exp,
        }
    }
}

pub struct PrivateKeyJwtSigner {
    private_key: String,
    algorithm: Algorithm, // Algorithm::RS256
}

impl PrivateKeyJwtSigner {
    pub fn new(private_key: String, algorithm: Algorithm) -> Self {
        Self {
            private_key,
            algorithm,
        }
    }
}

pub struct SignedJwt {
    expires_at: DateTime<Utc>,
    encoded_value: SignedJwtValue,
}

impl SignedJwt {
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub fn value(&self) -> SignedJwtValue {
        self.encoded_value.clone()
    }
}

impl JwtSigner for PrivateKeyJwtSigner {
    fn sign(&self, claims: Claims) -> Result<SignedJwt, JwtEncoderError> {
        let expires_at = Utc.timestamp_millis_opt(claims.exp as i64).unwrap();
        info!("Signing JWT with claims: {:?}", claims);
        let encoded_value = encode(
            &Header::new(self.algorithm),
            &claims,
            &EncodingKey::from_rsa_pem(&self.private_key.as_bytes())?,
        )?;

        Ok(SignedJwt {
            expires_at,
            encoded_value,
        })
    }
}

pub(super) fn jti() -> Jti {
    Ulid::new().to_string()
}
