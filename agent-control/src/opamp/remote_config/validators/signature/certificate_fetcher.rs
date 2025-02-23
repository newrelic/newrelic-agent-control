use super::certificate::Certificate;
use crate::http::client::HttpClient;
use reqwest::tls::TlsInfo;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use std::path::PathBuf;
use thiserror::Error;
use tracing::log::error;
use url::Url;

pub type DerCertificateBytes = Vec<u8>;

#[derive(Error, Debug)]
pub enum CertificateFetcherError {
    #[error("building client to fetch certificate: `{0}`")]
    FetchClientBuild(String),
    #[error("fetching certificate: `{0}`")]
    CertificateFetch(String),
}
pub enum CertificateFetcher {
    Https(Url, HttpClient),
    PemFile(PathBuf),
}

impl CertificateFetcher {
    pub fn fetch(&self) -> Result<Certificate, CertificateFetcherError> {
        let cert = match self {
            CertificateFetcher::Https(url, client) => CertificateFetcher::fetch_https(url, client)?,
            CertificateFetcher::PemFile(pem_file_path) => {
                CertificateFetcher::fetch_file(pem_file_path)?
            }
        };
        Certificate::try_new(cert)
            .map_err(|e| CertificateFetcherError::CertificateFetch(e.to_string()))
    }

    fn fetch_https(
        url: &Url,
        client: &HttpClient,
    ) -> Result<DerCertificateBytes, CertificateFetcherError> {
        let request = http::Request::builder()
            .uri(url.as_ref())
            .method("HEAD")
            .body(Vec::default())
            .map_err(|err| {
                CertificateFetcherError::CertificateFetch(format!(
                    "error building request: {}",
                    err
                ))
            })?;
        let response = client.send(request).map_err(|e| {
            CertificateFetcherError::CertificateFetch(format!("fetching certificate: {}", e))
        })?;
        let tls_info = response.extensions().get::<TlsInfo>().ok_or(
            CertificateFetcherError::CertificateFetch("missing tls information".to_string()),
        )?;

        let leaf_cert_der =
            tls_info
                .peer_certificate()
                .ok_or(CertificateFetcherError::CertificateFetch(
                    "missing leaf certificates".into(),
                ))?;

        Ok(leaf_cert_der.to_vec())
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
    use std::time::Duration;

    use super::*;
    use crate::http::config::HttpConfig;
    use crate::http::config::ProxyConfig;
    use crate::http::tls::install_rustls_default_crypto_provider;
    use crate::opamp::remote_config::validators::signature::certificate_store::tests::TestSigner;
    use assert_matches::assert_matches;

    const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

    #[test]
    fn test_https_fetcher() {
        install_rustls_default_crypto_provider();

        struct TestCase {
            name: &'static str,
            url: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let http_config = HttpConfig::new(
                    DEFAULT_CLIENT_TIMEOUT,
                    DEFAULT_CLIENT_TIMEOUT,
                    ProxyConfig::default(),
                )
                .with_tls_info();
                let client = HttpClient::new(http_config).unwrap();
                let _ = CertificateFetcher::Https(Url::parse(self.url).unwrap(), client)
                    .fetch()
                    .unwrap_or_else(|err| {
                        panic!("fetching cert err '{}', case: '{}'", err, self.name)
                    });
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
                let http_config = HttpConfig::new(
                    DEFAULT_CLIENT_TIMEOUT,
                    DEFAULT_CLIENT_TIMEOUT,
                    ProxyConfig::default(),
                )
                .with_tls_info();
                let client = HttpClient::new(http_config).unwrap();
                let err = CertificateFetcher::Https(Url::parse(self.url).unwrap(), client)
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
