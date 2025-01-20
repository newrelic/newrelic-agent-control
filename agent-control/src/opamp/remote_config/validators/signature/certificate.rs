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
            public_key_id: public_key_fingerprint(cer.public_key().raw),
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
}

pub fn public_key_fingerprint(public_key: &[u8]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_certificate_key_id() {
        let mut cursor = Cursor::new(CERT_PEM.as_bytes());
        let cert = rustls_pemfile::certs(cursor.get_mut())
            .next()
            .unwrap()
            .unwrap();

        let key_id = Certificate::try_new(cert.as_ref().to_vec())
            .unwrap()
            .public_key_id()
            .to_string();

        assert_eq!(key_id, CERT_PUBLIC_KEY_ID);
    }

    const CERT_PUBLIC_KEY_ID: &str =
        "3c333a786b8f1e93f3a099a09cf591c5faac126ea48699e9e290e72b0b6bf06c";
    const CERT_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDLzCCAhegAwIBAgIUc5RF25ZGKeFSMlB8EK0EuDxZUFgwDQYJKoZIhvcNAQEL
BQAwHjELMAkGA1UEBhMCRkkxDzANBgNVBAMMBmNhbmFtZTAeFw0yNTAxMjAwNzU2
NTBaFw0yNjAxMjAwNzU2NTBaMCAxCzAJBgNVBAYTAkZJMREwDwYDVQQDDAh0ZXN0
bmFtZTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAL6pXsPX+0HpRdp+
xD88Ut/SL26kmYSCaY9U1nCo45bARlTlhW62Bf5WMETJhGGi/Kq93MjPMkmNFNF/
2qQx+XpxmKQR+B/iQzrg9bD1evRQPQvnSFBHKMh8cbqVpsLq/p6ee2iMoDpQ8C8p
Y1WjmGhcpp7EpDLUwx2x8NOu+uZp7NjT2rFBni7KMcWKJXEYh59EHkL/J/DeTUtQ
0Jxrq6k2hbEBOxRzO3XdwZ3w+LlurankJBOBljLpXn7Du9iA/0BicWczBhwJqv3T
96gyxoClmyGpXRiaiHyP+6t7/xfNfwJ6AEuifyVIUnxEyP+lgx6stWnV2j58a4kT
asRIASECAwEAAaNjMGEwHwYDVR0jBBgwFoAUwg0OUU2UnO8UnMGFAjUdIl2S5Jow
CQYDVR0TBAIwADAUBgNVHREEDTALgglsb2NhbGhvc3QwHQYDVR0OBBYEFLoNRu6n
UepmUndgCwPr7tHQ84N0MA0GCSqGSIb3DQEBCwUAA4IBAQB4yKCYrdbz4FGxfA4K
GbgXe0ylio1OCA/4Db3Xo/UYJwKG+sG5YWKJiOTqJqdOPSczZE8ROA9BNLKpfUXj
hIffqUXca298j+8Ag+gFE5oOnUF1RUwE+xLWj94Fby4yFeadPcn1E7amSGoK1kE2
ksQmmplpaVP9lOKnk6pX9NbMsAW2IeuDROuCYyTE9XOUxzdNnQp2Uk7rnxbGHHIl
ag5JWpNv/SRwijhGyVKiLFINYILDaNZc56RxxNWgfKj8mTiRvFV5OiM0MrIjBCUu
O0jhqIc+AEbSGU0jdfFxs4f9fJklHDphUxqE1MSvqzOMaFNrt/8jEupa2ujLCVId
XeFA
-----END CERTIFICATE-----"#;
}
