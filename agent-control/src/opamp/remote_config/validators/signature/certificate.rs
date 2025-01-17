use ring::digest;
use std::fmt::Write;
use thiserror::Error;
use webpki::EndEntityCert;
use x509_parser::prelude::{FromDer, X509Certificate};

#[derive(Error, Debug)]
pub enum CertificateError {
    #[error("parsing certificate from bytes: `{0}`")]
    ParseCertificate(String),
    #[error("verifying signature: `{0}`")]
    VerifySignature(String),
}
#[derive(Debug, Clone)]
pub struct Certificate {
    cert_der: Vec<u8>,
    // sha256 digest of the public key
    public_key_id: String,
}

impl Certificate {
    pub fn try_new(cert_der: Vec<u8>) -> Result<Self, CertificateError> {
        let (_, cer) = X509Certificate::from_der(&cert_der)
            .map_err(|e| CertificateError::ParseCertificate(e.to_string()))?;

        Ok(Self {
            public_key_id: Self::digest_sha256(cer.public_key().raw),
            cert_der,
        })
    }
    pub fn public_key_id(&self) -> &str {
        &self.public_key_id
    }
    pub fn verify_signature(
        &self,
        algorithm: &webpki::SignatureAlgorithm,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), CertificateError> {
        let certificate = EndEntityCert::try_from(self.cert_der.as_slice())
            .map_err(|e| CertificateError::VerifySignature(e.to_string()))?;

        certificate
            .verify_signature(algorithm, msg, signature)
            .map_err(|e| CertificateError::VerifySignature(e.to_string()))
    }

    fn digest_sha256(public_key: &[u8]) -> String {
        let key_id_bytes = digest::digest(&digest::SHA256, public_key);

        // encode the digest as hex string
        key_id_bytes
            .as_ref()
            .iter()
            .fold(String::new(), |mut output, b| {
                let _ = write!(output, "{b:02x}");
                output
            })
    }
}
