use super::certificate::Certificate;
use crate::http::tls::root_store_with_native_certs;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use rustls::ClientConnection;
use rustls::Stream;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::log::error;
use url::Url;

pub type DerCertificateBytes = Vec<u8>;
pub type ConnectionTimeout = Duration;

const HEAD_REQUEST: &str = "HEAD / HTTP/1.1\r\n";

#[derive(Error, Debug)]
pub enum CertificateFetcherError {
    #[error("building client to fetch certificate: `{0}`")]
    FetchClientBuild(String),
    #[error("fetching certificate: `{0}`")]
    CertificateFetch(String),
}
pub enum CertificateFetcher {
    Https(Url, ConnectionTimeout),
    PemFile(PathBuf),
}

impl CertificateFetcher {
    pub fn fetch(&self) -> Result<Certificate, CertificateFetcherError> {
        let cert = match self {
            CertificateFetcher::Https(url, connection_timeout) => {
                CertificateFetcher::fetch_https(url, connection_timeout)?
            }
            CertificateFetcher::PemFile(pem_file_path) => {
                CertificateFetcher::fetch_file(pem_file_path)?
            }
        };
        Certificate::try_new(cert)
            .map_err(|e| CertificateFetcherError::CertificateFetch(e.to_string()))
    }

    fn fetch_https(
        url: &Url,
        connection_timeout: &ConnectionTimeout,
    ) -> Result<DerCertificateBytes, CertificateFetcherError> {
        let root_store_with_native_certs = root_store_with_native_certs()
            .map_err(|e| CertificateFetcherError::FetchClientBuild(e.to_string()))?;
        let config = ClientConfig::builder()
            .with_root_certificates(root_store_with_native_certs)
            .with_no_client_auth();

        // Server where the certificate is fetched from. The server name is used to validate the certificate by the client.
        let server_name = CertificateFetcher::server_name(url)?;

        let mut conn: ClientConnection = ClientConnection::new(Arc::new(config), server_name)
            .map_err(|e| {
                CertificateFetcherError::FetchClientBuild(format!(
                    "creating ClientConnection: {}",
                    e
                ))
            })?;

        // Url can resolve to multiple addresses, try each one until we get a certificate
        let addrs = url.socket_addrs(|| None).map_err(|e| {
            CertificateFetcherError::FetchClientBuild(format!("creating address from url: {}", e))
        })?;

        let mut last_error = None;
        for addr in addrs {
            match CertificateFetcher::fetch_certificate_from_address(
                &addr,
                &mut conn,
                connection_timeout,
            ) {
                Ok(cert) => return Ok(cert),
                Err(e) => {
                    error!("error fetching certificate from address: {}", e);
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            CertificateFetcherError::CertificateFetch(
                "could not resolve to any address".to_string(),
            )
        }))
    }

    fn fetch_certificate_from_address(
        addr: &SocketAddr,
        conn: &mut ClientConnection,
        connection_timeout: &ConnectionTimeout,
    ) -> Result<DerCertificateBytes, CertificateFetcherError> {
        let mut stream = TcpStream::connect_timeout(addr, *connection_timeout).map_err(|e| {
            CertificateFetcherError::CertificateFetch(format!("to connect to address: {}", e))
        })?;
        let mut tls = Stream::new(conn, &mut stream);
        // send a simple HTTP request just to establish the connection so TLS handshake can happen,
        // and the ClientConnection can get the peer certificates.
        tls.write_all(HEAD_REQUEST.as_bytes()).map_err(|e| {
            CertificateFetcherError::CertificateFetch(format!("establishing tls connection: {}", e))
        })?;

        let certificates_chain =
            conn.peer_certificates()
                .ok_or(CertificateFetcherError::CertificateFetch(
                    "missing peer certificates".into(),
                ))?;

        // First certificate in the chain is the leaf certificate, which is the one used to sign the config.
        let leaf_cert_der =
            certificates_chain
                .first()
                .ok_or(CertificateFetcherError::CertificateFetch(
                    "missing leaf certificate".into(),
                ))?;

        Ok(leaf_cert_der.as_ref().to_vec())
    }

    fn server_name<'a>(url: &Url) -> Result<ServerName<'a>, CertificateFetcherError> {
        let domain = url
            .domain()
            .ok_or(CertificateFetcherError::FetchClientBuild(format!(
                "parsing domain {}",
                url
            )))?;

        let server_name = ServerName::try_from(domain)
            .map_err(|e| {
                CertificateFetcherError::FetchClientBuild(format!(
                    "parsing ServerName from domain {}: {}",
                    url, e
                ))
            })?
            .to_owned();
        Ok(server_name)
    }

    fn fetch_file(pem_file_path: &PathBuf) -> Result<DerCertificateBytes, CertificateFetcherError> {
        let cert = CertificateDer::from_pem_file(pem_file_path).map_err(|e| {
            CertificateFetcherError::CertificateFetch(format!(
                "reading certificate from file: {}",
                e
            ))
        })?;
        Ok(cert.as_ref().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::tls::install_rustls_default_crypto_provider;
    use crate::opamp::remote_config::validators::signature::certificate_store::tests::TestSigner;
    use assert_matches::assert_matches;

    #[test]
    fn test_https_fetcher() {
        install_rustls_default_crypto_provider();

        struct TestCase {
            name: &'static str,
            url: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let _ = CertificateFetcher::Https(
                    Url::parse(self.url).unwrap(),
                    Duration::from_secs(10),
                )
                .fetch()
                .unwrap_or_else(|err| panic!("fetching cert err '{}', case: '{}'", err, self.name));
            }
        }
        let test_cases = vec![
            TestCase {
                name: "rsa sha256",
                url: "https://sha256.badssl.com/",
            },
            TestCase {
                name: "ecc sha256",
                url: "https://ecc256.badssl.com/",
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
    #[test]
    fn test_https_fetcher_fails() {
        install_rustls_default_crypto_provider();

        struct TestCase {
            name: &'static str,
            url: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let err = CertificateFetcher::Https(
                    Url::parse(self.url).unwrap(),
                    Duration::from_secs(10),
                )
                .fetch()
                .expect_err(format!("error is expected, case: {}", self.name).as_str());

                assert_matches!(err, CertificateFetcherError::CertificateFetch(_));
            }
        }
        let test_cases = vec![
            TestCase {
                name: "missing endpoint",
                url: "https://badssl.com:9999/",
            },
            TestCase {
                name: "http",
                url: "http://http.badssl.com/",
            },
            TestCase {
                name: "expired certificate",
                url: "https://expired.badssl.com/",
            },
            TestCase {
                name: "wrong host",
                url: "https://wrong.host.badssl.com/",
            },
            TestCase {
                name: "untrusted root",
                url: "https://untrusted-root.badssl.com/",
            },
            TestCase {
                name: "self signed",
                url: "https://self-signed.badssl.com/",
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_file_fetcher() {
        let test_signer = TestSigner::new();
        CertificateFetcher::PemFile(test_signer.cert_pem_path())
            .fetch()
            .expect("to fetch certificate");
    }
}
