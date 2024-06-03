use std::{io, path::Path};

use chrono::{offset::LocalResult, TimeZone, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use thiserror::Error;

use crate::jwt::{claims::Claims, error::JwtEncoderError, signed::SignedJwt};

use super::JwtSigner;

/// Errors that can occur when creating a LocalPrivateKeySigner.
#[derive(Debug, Error)]
pub enum LocalPrivateKeySignerError {
    #[error("unable to load private key: `{0}`")]
    Encoding(#[from] jsonwebtoken::errors::Error),
    #[error("filesystem i/o error: `{0}`")]
    IO(#[from] io::Error),
}

/// Signer structure that uses a local private key to sign JWTs.
pub struct LocalPrivateKeySigner {
    encoding_key: EncodingKey,
    algorithm: Algorithm,
}

/// Attempt to create a LocalPrivateKeySigner from a PemFileContents.
impl TryFrom<Vec<u8>> for LocalPrivateKeySigner {
    type Error = LocalPrivateKeySignerError;

    fn try_from(pem: Vec<u8>) -> Result<Self, Self::Error> {
        // Algorithm is hardcoded to RS256, so decoding key is also fixed. Here we load it from
        // a PEM file.
        Ok(Self {
            // This will call pem::parse
            encoding_key: EncodingKey::from_rsa_pem(pem.as_ref())?,
            algorithm: Algorithm::RS256,
        })
    }
}

// Attempt to create a LocalPrivateKeySigner from a file path, which must be a valid PEM file.
impl TryFrom<&Path> for LocalPrivateKeySigner {
    type Error = LocalPrivateKeySignerError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let pem = std::fs::read(path)?;
        Self::try_from(pem)
    }
}

/// Sign a JWT using a local private key.
impl JwtSigner for LocalPrivateKeySigner {
    // Algorithm-agnostic, though we only support RS256.
    // Change the algorithm and encoding key in LocalPrivateKeySigner to support other.
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
        let value = jsonwebtoken::encode(&Header::new(self.algorithm), &claims, &self.encoding_key)
            .map_err(|e| JwtEncoderError::TokenEncoding(e.to_string()))?;
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
    use url::Url;

    const RS256_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEuwIBADANBgkqhkiG9w0BAQEFAASCBKUwggShAgEAAoIBAQC2PaghXmD7Sctw
HHkkF3yDkBlemb1qWKt6Io8GW7OlYSJ60HDJtJXrQ3woIcKgr1ammaXE1aMliUHW
LclLvh5x00e6eNpTrnKEpXrhe139VM2QrgGwp2glNHttTEbTExLBHSEcY6tx6g4Z
D3pIlKLYpqWwCo8IsUuvJpwHeHQG8rJt7JKeQg71D8mZdPWVp8Hafm9e/Zs5CSzA
5CF0bujLBRQGlgMHRIr7hpXXZ3RoeiUFC+yW0VMvVfhd3bWHx4IVy3K6rusbAy0z
W9yUsaYGs+QHzKtmMlT9+kXYPofMZ+KcpFugFNyajuZQXbC5gBGP8iy4SSWHSDPu
ux4h/sblAgMBAAECggEAFu48ptA3jz7qknV+t7Ad2ncJ/imFmClGkFRjXzcwLE3D
1yS9oF+w4nyoFWukD/BoDIf2QAVqpRk3d8Hkm3t1XLirRJcaz586aR7iTpdljO/7
+qmubEIwPEg1hJvtqHb0q+hkp2wSIUAEXJpiNlo/gFe9ruAxPbSDU6tdxCHfpZTz
SlZSa0mwcAuKVuq6chdtLurvvVTLatI2/Avg22tkVRfjyGe4NKNak3N09htmtt3k
nxzsDz229Ho7Qw0lEU/Rpo60p/1UFSLH5Kdsycc33cF0ACznAQ3pWozkwXVR0TfF
rmUFX73/zZfI3/expjuk3HTUZ/6W4mHgZZA6oqUHAQKBgQDbQsCr/SxdtrKx8VL5
xwMIxamVxePkKH9+P3m+bw8xaT6buyrX1Y/kkyyEBqRd9W6iiKEFF7h1Or2uKjqh
5WoKPASh8AFVtAeTgtWQWRN2+iLt4jTIxnbzeUiNFCLY5hFTZnpM64vkOeNx1lfd
Uhet30/x35TRgbyU2pIQ9lOz5QKBgQDUxuzzTLnXKDbRd3fxLhnqNMuz2PUvAkTQ
zyuqIHHUqEMx1oFaslAlFSjX+FEhEuOqISlDZf09OYvnSRF9fz3ronm3yYGxPBVr
rwpE9lGdsy/ul1/EU3FjsVAZ0MOf+1RB69xoMrYTi9+CfEF9Ue5zqMIN/ibgyx6V
souIn2OXAQKBgQC8PKq8/TXBnr/7FHtwBPMN7OSSuLnVfw81i7kxTJd2jCw79ovp
kGdgjRmCn1EteS/qSfIzNRIfUrbVd1uu8g3/i1dOz4XV1iFK+t/udQrI8iZapAE8
/WXR0SYAOHFSVPI675e/wdjvruMdMC9uyrOZikZQGOrikscb5CnSdieWIQJ/S6Jq
mBGt/c1NryfIevLoQ1iBEG0OuqcTzyXVX6Qo0m79c7nMQXEhDA15d0vNivQr+U3Q
XSTj39+U26IdlX6lhB09Jxd6AoZZFu4huGHWoTgQ0b79S8xdghKFZqfO4g904/nz
XxanoksWKEwC+4kkOfjDAjZVm5KYTJ4q+2WtAQKBgHeeQfmvoCzCpPpZD/K2SxGD
sJWFhh7HSGFHc3C9bJ5KrftA0o64SeXEGSnxFQJ2oGrLqlZuyfdJ0NsDI9kQVWnM
USEqOAWZjvEBorOcB1tTO3vBgZOBz41i/x9xlYw2fmt+fTBUNAN6ABFcrEEaAIFQ
3PdAPhldn/zZaxkLJ4h1
-----END PRIVATE KEY-----
"#;

    const RS256_PUBLIC_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtj2oIV5g+0nLcBx5JBd8
g5AZXpm9alireiKPBluzpWEietBwybSV60N8KCHCoK9WppmlxNWjJYlB1i3JS74e
cdNHunjaU65yhKV64Xtd/VTNkK4BsKdoJTR7bUxG0xMSwR0hHGOrceoOGQ96SJSi
2KalsAqPCLFLryacB3h0BvKybeySnkIO9Q/JmXT1lafB2n5vXv2bOQkswOQhdG7o
ywUUBpYDB0SK+4aV12d0aHolBQvsltFTL1X4Xd21h8eCFctyuq7rGwMtM1vclLGm
BrPkB8yrZjJU/fpF2D6HzGfinKRboBTcmo7mUF2wuYARj/IsuEklh0gz7rseIf7G
5QIDAQAB
-----END PUBLIC KEY-----
"#;

    #[test]
    fn local_private_key_signer_rsa() {
        // Set expected claims content
        let audience = Url::parse("http://127.0.0.1/").unwrap();
        let client_id = "test"; // For both issuer and subject

        // Claims
        let claims = Claims::new(
            client_id.to_owned(),
            audience.clone(),
            get_current_timestamp(),
        );

        // Validation
        let mut validation = Validation::new(Algorithm::RS256);
        validation.sub = Some(client_id.to_owned());
        validation.set_audience(&[audience.clone()]);
        validation.set_required_spec_claims(&["exp", "sub", "aud"]);

        // Create local signer
        let signer =
            LocalPrivateKeySigner::try_from(RS256_PRIVATE_KEY.as_bytes().to_vec()).unwrap();

        // Sign the token
        let signed_jwt = signer.sign(claims);
        assert!(signed_jwt.is_ok());

        // Decode the signed token
        let token = signed_jwt.unwrap();
        let decoded = jsonwebtoken::decode::<Claims>(
            &token.value,
            &DecodingKey::from_rsa_pem(RS256_PUBLIC_KEY.as_bytes()).unwrap(),
            &validation,
        );

        // Assertions
        assert!(decoded.is_ok());

        let decoded_claims = decoded.unwrap().claims;
        assert_eq!(decoded_claims.sub, "test");
        assert_eq!(decoded_claims.aud, audience.to_string());
    }

    #[test]
    fn bad_pem_file() {
        let signer = LocalPrivateKeySigner::try_from("WRONG".as_bytes().to_vec());
        assert!(signer.is_err());
    }
}
