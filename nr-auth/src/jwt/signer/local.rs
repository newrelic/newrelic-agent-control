use chrono::{offset::LocalResult, TimeZone, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};

use crate::jwt::{claims::Claims, error::JwtEncoderError, signed::SignedJwt};

use super::JwtSigner;

#[derive(Debug)]
struct LocalPrivateKeySignerBuilder {
    private_key: Vec<u8>,
    algorithm: Algorithm,
}

impl LocalPrivateKeySignerBuilder {
    fn with_private_key(self, private_key: Vec<u8>) -> Self {
        Self {
            private_key,
            ..self
        }
    }

    fn with_algorithm(self, algorithm: Algorithm) -> Self {
        Self { algorithm, ..self }
    }
}

impl Default for LocalPrivateKeySignerBuilder {
    fn default() -> Self {
        Self {
            private_key: Vec::default(), // empty private key
            algorithm: Algorithm::RS256, // default algorithm: RSASSA-PKCS1-v1_5 using SHA-256. // FIXME?
        }
    }
}

struct LocalPrivateKeySigner {
    encoding_key: EncodingKey,
    algorithm: Algorithm,
}

impl TryFrom<LocalPrivateKeySignerBuilder> for LocalPrivateKeySigner {
    type Error = JwtEncoderError;

    fn try_from(builder: LocalPrivateKeySignerBuilder) -> Result<Self, Self::Error> {
        let encoding_key = EncodingKey::from_rsa_pem(&builder.private_key)?;
        Ok(Self {
            encoding_key,
            algorithm: builder.algorithm,
        })
    }
}

impl JwtSigner for LocalPrivateKeySigner {
    fn sign(&self, claims: Claims) -> Result<SignedJwt, JwtEncoderError> {
        let expiration_date = match Utc.timestamp_millis_opt(claims.exp as i64) {
            // able to retrieve a single value, correct timestamp
            LocalResult::Single(date) => date,
            // the variants below deal with unusual timestamp values due to daylight saving time
            // I'm not sure of the implications of this regarding security, so for the moment we only
            // accept a single value for the timestamp.

            // the ambiguous time result happens when the clock is turned backwards during a transition for example due to daylight saving time
            LocalResult::Ambiguous(earliest, latest) => {
                return Err(JwtEncoderError::InvalidTimestamp(format!(
                    "ambiguous timestamp. Earliest: {earliest}, Latest: {latest}"
                )))
            }
            // the none result happens when the clock is turned forwards during a transition for example due to daylight saving time
            LocalResult::None => {
                return Err(JwtEncoderError::InvalidTimestamp(
                    "invalid timestamp was provided".to_owned(),
                ))
            }
        };
        let value =
            jsonwebtoken::encode(&Header::new(self.algorithm), &claims, &self.encoding_key)?;
        Ok(SignedJwt {
            expiration_date,
            value,
        })
    }
}
