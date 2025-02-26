use crate::cloud::http_client::{try_build_response, HttpClient, HttpClientError};
use core::str;
use reqwest::blocking::Client;
use std::time::Duration;

pub(crate) const TOKEN_HEADER: &str = "x-aws-ec2-metadata-token";
pub(crate) const TTL_TOKEN_HEADER: &str = "x-aws-ec2-metadata-token-ttl-seconds";

/// An implementation of the `HttpClient` trait using the reqwest library and IMDv2 auth.
pub struct AWSHttpClientReqwest {
    http_client: Client,
    token_endpoint: String,
    token_ttl: Duration,
    metadata_endpoint: String,
}

impl AWSHttpClientReqwest {
    /// Returns a new instance of AWSHttpClientReqwest
    pub fn try_new(
        http_client: Client,
        metadata_endpoint: String,
        token_endpoint: String,
        token_ttl: Duration,
        timeout: Duration,
    ) -> Result<Self, HttpClientError> {
        Ok(Self {
            http_client,
            metadata_endpoint,
            token_endpoint,
            token_ttl,
        })
    }

    fn get_token(&self) -> Result<String, HttpClientError> {
        let response = self
            .http_client
            .put(self.token_endpoint.as_str())
            .header(
                TTL_TOKEN_HEADER,
                self.token_ttl.as_secs().to_string().as_str(),
            )
            .send()?;

        if !response.status().is_success() {
            return Err(HttpClientError::ResponseError(
                response.status().into(),
                response.text()?,
            ));
        }

        let bytes = response.bytes()?;
        let token = str::from_utf8(bytes.as_ref())
            .map_err(|err| {
                HttpClientError::TransportError(format!("could not decode AWS IMDS token {err}"))
            })?
            .to_string();
        Ok(token)
    }
}

impl HttpClient for AWSHttpClientReqwest {
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        let token = self.get_token()?;

        let req = self
            .http_client
            .get(&self.metadata_endpoint)
            .header(TOKEN_HEADER, &token);

        let response = req.send()?;

        if !response.status().is_success() {
            return Err(HttpClientError::ResponseError(
                response.status().into(),
                response.text()?,
            ));
        }

        try_build_response(response)
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud::aws::http_client::{AWSHttpClientReqwest, TOKEN_HEADER, TTL_TOKEN_HEADER};
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

        let http_client = AWSHttpClientReqwest::try_new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        )
        .unwrap();

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

        let http_client = AWSHttpClientReqwest::try_new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        )
        .unwrap();

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

        let http_client = AWSHttpClientReqwest::try_new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        )
        .unwrap();

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

        let http_client = AWSHttpClientReqwest::try_new(
            imds_server.url("/metadata"),
            imds_server.url("/token"),
            TTL_TOKEN,
            DEFAULT_CLIENT_TIMEOUT,
        )
        .unwrap();

        let err = http_client.get().unwrap_err();

        assert_matches!(err, HttpClientError::TransportError(_));

        token_mock.assert_hits(1);
    }
}
