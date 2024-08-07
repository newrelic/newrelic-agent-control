use crate::cloud::http_client::{HttpClient, HttpClientError};
use std::time::Duration;

pub(crate) const TOKEN_HEADER: &str = "x-aws-ec2-metadata-token";
pub(crate) const TTL_TOKEN_HEADER: &str = "x-aws-ec2-metadata-token-ttl-seconds";

/// An implementation of the `HttpClient` trait using the ureq library and IMDv2 auth.
pub struct AWSHttpClientUreq {
    http_client: ureq::Agent,
    token_endpoint: String,
    token_ttl: Duration,
    metadata_endpoint: String,
}

impl AWSHttpClientUreq {
    /// Returns a new instance of AWSHttpClientUreq
    pub fn new(
        metadata_endpoint: String,
        token_endpoint: String,
        token_ttl: Duration,
        timeout: Duration,
    ) -> Self {
        Self {
            http_client: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout(timeout)
                .build(),
            metadata_endpoint,
            token_endpoint,
            token_ttl,
        }
    }

    fn get_token(&self) -> Result<String, HttpClientError> {
        let response = self
            .http_client
            .put(self.token_endpoint.as_str())
            .set(
                TTL_TOKEN_HEADER,
                self.token_ttl.as_secs().to_string().as_str(),
            )
            .call()?;

        let token = response.into_string().map_err(|err| {
            HttpClientError::InternalError(format!("getting AWS IMDS token: {}", err))
        })?;

        Ok(token)
    }
}

impl HttpClient for AWSHttpClientUreq {
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        let token = self.get_token()?;

        let req = self
            .http_client
            .get(&self.metadata_endpoint)
            .set(TOKEN_HEADER, &token);

        Ok(req.call()?.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud::aws::http_client::{AWSHttpClientUreq, TOKEN_HEADER, TTL_TOKEN_HEADER};
    use crate::cloud::http_client::{HttpClient, HttpClientError, DEFAULT_CLIENT_TIMEOUT};
    use assert_matches::assert_matches;
    use httpmock::MockServer;
    use std::time::Duration;

    const TTL_TOKEN: Duration = Duration::from_secs(10);

    #[test]
    fn test_authenticated_request() {
        let imds_server = MockServer::start();
        let token_mock = imds_server.mock(|when, then| {
            when.method("PUT")
                .path("/token")
                .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
            then.status(200).body("test_token");
        });
        let metadata_mock = imds_server.mock(|when, then| {
            when.method("GET")
                .path("/metadata")
                .header(TOKEN_HEADER, "test_token");
            then.status(200).body("test_metadata");
        });

        let http_client = AWSHttpClientUreq::new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        );

        let resp = http_client.get().unwrap();

        assert_eq!(resp.body(), b"test_metadata");

        token_mock.assert_hits(1);
        metadata_mock.assert_hits(1);
    }

    #[test]
    fn test_failed_metadata_endpoint() {
        let imds_server = MockServer::start();
        let token_mock = imds_server.mock(|when, then| {
            when.method("PUT")
                .path("/token")
                .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
            then.status(200).body("test_token");
        });
        let metadata_mock = imds_server.mock(|when, then| {
            when.method("GET")
                .path("/metadata")
                .header(TOKEN_HEADER, "test_token");
            then.status(401).body("test_metadata");
        });

        let http_client = AWSHttpClientUreq::new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        );

        let err = http_client.get().unwrap_err();

        assert_matches!(err, HttpClientError::ResponseError(401, _));

        token_mock.assert_hits(1);
        metadata_mock.assert_hits(1);
    }

    #[test]
    fn test_fail_getting_token() {
        let imds_server = MockServer::start();
        let token_mock = imds_server.mock(|when, then| {
            when.method("PUT")
                .path("/token")
                .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
            then.status(500);
        });

        let http_client = AWSHttpClientUreq::new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        );

        let err = http_client.get().unwrap_err();

        assert_matches!(err, HttpClientError::ResponseError(500, _));

        token_mock.assert_hits(1);
    }
    #[test]
    fn test_fail_deserializing_token() {
        let imds_server = MockServer::start();
        let invalid_token = vec![0x89, 0x50, 0x4E, 0x47];

        let token_mock = imds_server.mock(|when, then| {
            when.method("PUT")
                .path("/token")
                .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
            then.status(200).body(invalid_token);
        });

        let http_client = AWSHttpClientUreq::new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        );

        let err = http_client.get().unwrap_err();

        assert_matches!(err, HttpClientError::TransportError(_));

        token_mock.assert_hits(1);
    }
}
