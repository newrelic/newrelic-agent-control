use chrono::{offset::LocalResult, TimeZone, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};

use crate::jwt::{claims::Claims, error::JwtEncoderError, signed::SignedJwt};

use super::JwtSigner;

#[derive(Debug)]
pub struct LocalPrivateKeySignerConfig {
    pub private_key: Vec<u8>,
    pub algorithm: Algorithm,
}

pub struct LocalPrivateKeySigner {
    encoding_key: EncodingKey,
    algorithm: Algorithm, // TODO what algos should we support?
}

impl TryFrom<LocalPrivateKeySignerConfig> for LocalPrivateKeySigner {
    type Error = JwtEncoderError;

    fn try_from(builder: LocalPrivateKeySignerConfig) -> Result<Self, Self::Error> {
        let encoding_key = {
            use Algorithm::*;
            match builder.algorithm {
                HS256 | HS384 | HS512 => EncodingKey::from_secret(builder.private_key.as_ref()),
                ES256 | ES384 => EncodingKey::from_ec_pem(builder.private_key.as_ref())?,
                EdDSA => EncodingKey::from_ed_pem(builder.private_key.as_ref())?,
                RS256 | RS384 | RS512 | PS256 | PS384 | PS512 => {
                    EncodingKey::from_rsa_pem(builder.private_key.as_ref())?
                }
            }
        };

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

#[cfg(test)]
mod test {
    use super::*;
    use jsonwebtoken::{get_current_timestamp, DecodingKey, Validation};

    const ED25519_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIDsQN721qUT+IHzmkDDx6+Oqwi83yLhznh7tmjnrCdW1
-----END PRIVATE KEY-----"#;

    const ED25519_PUBLIC_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAUm7btVmPKCeaBDWIWUz5rL5hUkqlKPjX9z5kMfxgEns=
-----END PUBLIC KEY-----"#;

    #[test]
    fn local_private_key_signer_hmac() {
        // Claims
        let claims = Claims::new(get_current_timestamp())
            .with_subject("test".to_owned())
            .with_audience("test".to_owned());

        // Validation
        let mut validation = Validation::new(Algorithm::HS256);
        validation.sub = Some("test".to_owned());
        validation.set_audience(&["test"]);
        validation.set_required_spec_claims(&["exp", "sub", "aud"]);

        // Create local signer
        let signer_builder = LocalPrivateKeySignerConfig {
            private_key: b"secret".to_vec(),
            algorithm: Algorithm::HS256, // secret-based
        };
        let signer = LocalPrivateKeySigner::try_from(signer_builder).unwrap();

        // Sign the token
        let signed_jwt = signer.sign(claims);
        assert!(signed_jwt.is_ok());

        // Decode the signed token
        let token = signed_jwt.unwrap();
        let decoded = jsonwebtoken::decode::<Claims>(
            &token.value,
            &DecodingKey::from_secret(b"secret"),
            &validation,
        );
        // Assertions
        assert!(decoded.is_ok());

        let decoded_claims = decoded.unwrap().claims;
        assert_eq!(decoded_claims.sub, "test");
        assert_eq!(decoded_claims.aud, "test");
    }

    // Other algorithms that take PEM files should work the same way, so we only test this one.
    #[test]
    fn local_private_key_signer_pem_ecdsa() {
        // Claims
        let claims = Claims::new(get_current_timestamp())
            .with_subject("test".to_owned())
            .with_audience("test".to_owned());

        // Validation
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.sub = Some("test".to_owned());
        validation.set_audience(&["test"]);
        validation.set_required_spec_claims(&["exp", "sub", "aud"]);

        // Create local signer
        let signer_builder = LocalPrivateKeySignerConfig {
            private_key: ED25519_PRIVATE_KEY.as_bytes().to_vec(),
            algorithm: Algorithm::EdDSA,
        };
        let signer = LocalPrivateKeySigner::try_from(signer_builder).unwrap();

        // Sign the token
        let signed_jwt = signer.sign(claims);
        assert!(signed_jwt.is_ok());

        // Decode the signed token
        let token = signed_jwt.unwrap();
        let decoded = jsonwebtoken::decode::<Claims>(
            &token.value,
            &DecodingKey::from_ed_pem(ED25519_PUBLIC_KEY.as_bytes()).unwrap(),
            &validation,
        );
        // Assertions
        assert!(decoded.is_ok());

        let decoded_claims = decoded.unwrap().claims;
        assert_eq!(decoded_claims.sub, "test");
        assert_eq!(decoded_claims.aud, "test");
    }
}
