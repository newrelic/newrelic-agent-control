use crate::cloud::http_client::{HttpClient, HttpClientError};
use core::str;
use http::{HeaderValue, Request, Response};
use std::time::Duration;

pub(crate) const TOKEN_HEADER: &str = "x-aws-ec2-metadata-token";
pub(crate) const TTL_TOKEN_HEADER: &str = "x-aws-ec2-metadata-token-ttl-seconds";

/// An implementation of the `HttpClient` trait using the reqwest library and IMDv2 auth.
pub struct AWSHttpClientReqwest<C: HttpClient> {
    http_client: C,
    token_endpoint: String,
    token_ttl: Duration,
    metadata_endpoint: String,
}
impl<C: HttpClient> AWSHttpClientReqwest<C> {
    /// Returns a new instance of AWSHttpClientReqwest
    pub fn try_new(
        http_client: C,
        metadata_endpoint: String,
        token_endpoint: String,
        token_ttl: Duration,
    ) -> Result<Self, HttpClientError> {
        Ok(Self {
            http_client,
            metadata_endpoint,
            token_endpoint,
            token_ttl,
        })
    }

    fn get_token(&self) -> Result<String, HttpClientError> {
        let mut request = Request::builder()
            .method("PUT")
            .uri(&self.token_endpoint)
            .body(Vec::new())?;
        request.headers_mut().insert(
            TTL_TOKEN_HEADER,
            HeaderValue::from_str(self.token_ttl.as_secs().to_string().as_str())
                .map_err(|e| HttpClientError::BuildingError(e.to_string()))?,
        );
        let response = self.send(request)?;

        let bytes = response.body();
        let token = str::from_utf8(bytes.as_ref())
            .map_err(|err| {
                HttpClientError::TransportError(format!("could not decode AWS IMDS token {err}"))
            })?
            .to_string();
        Ok(token)
    }

    pub fn get(&self) -> Result<Response<Vec<u8>>, HttpClientError> {
        let token = self.get_token()?;
        let mut request = Request::builder()
            .method("GET")
            .uri(&self.metadata_endpoint)
            .body(Vec::new())?;
        request.headers_mut().insert(
            TOKEN_HEADER,
            HeaderValue::from_str(&token)
                .map_err(|e| HttpClientError::BuildingError(e.to_string()))?,
        );
        let response = self.send(request)?;
        println!("{:?}", response);
        Ok(response)
    }
}

impl<C> HttpClient for AWSHttpClientReqwest<C>
where
    C: HttpClient,
{
    fn send(&self, request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, HttpClientError> {
        let response = self.http_client.send(request)?;
        if !response.status().is_success() {
            return Err(HttpClientError::ResponseError(
                response.status().into(),
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or_default()
                    .to_string(),
            ));
        }
        Ok(response)
    }
}
#[cfg(test)]
mod tests {
    use crate::cloud::aws::http_client::AWSHttpClientReqwest;
    use crate::cloud::http_client::tests::MockHttpClientMock;
    use crate::cloud::http_client::HttpClientError;
    use http::{Response, StatusCode};
    use std::time::Duration;

    const TTL_TOKEN: Duration = Duration::from_secs(10);

    #[test]
    fn test_authenticated_request_token() {
        let mut mock_http_client = MockHttpClientMock::new();

        let token_response = Response::builder()
            .status(StatusCode::OK)
            .body(b"test_token".to_vec())
            .unwrap();

        mock_http_client
            .expect_send()
            .times(1)
            .return_once(move |_| Ok(token_response));

        let client = AWSHttpClientReqwest::try_new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        )
        .unwrap();

        assert_eq!(client.get_token().unwrap(), "test_token");
    }

    #[test]
    fn test_authenticated_request_get() {
        let mut mock_http_client = MockHttpClientMock::new();

        let token_response = Response::builder()
            .status(StatusCode::OK)
            .body(b"test_token".to_vec())
            .unwrap();

        let metadata = Response::builder()
            .status(StatusCode::OK)
            .body(b"test_metadata".to_vec())
            .unwrap();

        let responses = vec![Ok(token_response), Ok(metadata)];

        mock_http_client.should_send_sequence(responses);

        let client = AWSHttpClientReqwest::try_new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        )
        .unwrap();

        assert_eq!(client.get().unwrap().body(), b"test_metadata");
    }

    #[test]
    fn test_authenticated_request_token_error() {
        let mut mock_http_client = MockHttpClientMock::new();

        let token_response = Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(b"invalid".to_vec())
            .unwrap();

        mock_http_client
            .expect_send()
            .times(1)
            .return_once(move |_| Ok(token_response));

        let client = AWSHttpClientReqwest::try_new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        )
        .unwrap();

        let result = client.get();

        if let Err(HttpClientError::ResponseError(code, message)) = result {
            assert_eq!(code, 400);
            assert_eq!(message, "Bad Request".to_string());
        }
    }

    #[test]
    fn test_authenticated_request_get_error() {
        let mut mock_http_client = MockHttpClientMock::new();

        let token_response = Response::builder()
            .status(StatusCode::OK)
            .body(b"test_token".to_vec())
            .unwrap();

        let metadata = Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(b"oo".to_vec())
            .unwrap();

        let responses = vec![Ok(token_response), Ok(metadata)];

        mock_http_client.should_send_sequence(responses);

        let client = AWSHttpClientReqwest::try_new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        )
        .unwrap();

        let result = client.get();

        if let Err(HttpClientError::ResponseError(code, message)) = result {
            assert_eq!(code, 403);
            assert_eq!(message, "Forbidden".to_string());
        }
    }
    // #[test]
    // fn test_authenticated_request() {
    //    let client_http = Client::new();
    //     let imds_server = MockServer::start();
    //     let token_mock = imds_server.mock(|when, then| {
    //         when.method("PUT")
    //             .path("/token")
    //             .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
    //         then.status(200).body("test_token");
    //     });
    //     let metadata_mock = imds_server.mock(|when, then| {
    //         when.method("GET")
    //             .path("/metadata")
    //             .header(TOKEN_HEADER, "test_token");
    //         then.status(200).body("test_metadata");
    //     });
    //
    //     let http_client = AWSHttpClientReqwest::try_new(
    //         client_http,
    //         imds_server.url("/metadata"),
    //         imds_server.url("/token"),
    //         TTL_TOKEN,
    //     )
    //     .unwrap();
    //
    //     let resp = http_client.get().unwrap();
    //
    //     assert_eq!(resp.body(), b"test_metadata");
    //
    //     token_mock.assert_hits(1);
    //     metadata_mock.assert_hits(1);
    // }

    // #[test]
    // fn test_failed_metadata_endpoint() {
    //     let http_client = Client::new();
    //     let imds_server = MockServer::start();
    //     let token_mock = imds_server.mock(|when, then| {
    //         when.method("PUT")
    //             .path("/token")
    //             .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
    //         then.status(200).body("test_token");
    //     });
    //     let metadata_mock = imds_server.mock(|when, then| {
    //         when.method("GET")
    //             .path("/metadata")
    //             .header(TOKEN_HEADER, "test_token");
    //         then.status(401).body("test_metadata");
    //     });
    //     let aws_http_client = AWSHttpClientReqwest::try_new(
    //         http_client,
    //         imds_server.url("/metadata"),
    //         imds_server.url("/token"),
    //         TTL_TOKEN,
    //     )
    //     .unwrap();
    //
    //     let err = aws_http_client.get();
    //
    //     assert_matches!(err, HttpClientError::ResponseError(401, _));
    //
    //     token_mock.assert_hits(1);
    //     metadata_mock.assert_hits(1);
    // }
    //
    // #[test]
    // fn test_fail_getting_token() {
    //     let client_mock = MockHttpClientMock::new();
    //     let imds_server = MockServer::start();
    //     let token_mock = imds_server.mock(|when, then| {
    //         when.method("PUT")
    //             .path("/token")
    //             .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
    //         then.status(500);
    //     });
    //
    //     let http_client = AWSHttpClientReqwest::try_new(
    //         client_mock,
    //         imds_server.url("/metadata"),
    //         imds_server.url("/token"),
    //         TTL_TOKEN,
    //     )
    //     .unwrap();
    //
    //     let err = http_client.get().unwrap_err();
    //
    //     assert_matches!(err, HttpClientError::ResponseError(500, _));
    //
    //     token_mock.assert_hits(1);
    // }
    // #[test]
    // fn test_fail_deserializing_token() {
    //     let client_mock = MockHttpClientMock::new();
    //     let imds_server = MockServer::start();
    //     let invalid_token = vec![0x89, 0x50, 0x4E, 0x47];
    //
    //     let token_mock = imds_server.mock(|when, then| {
    //         when.method("PUT")
    //             .path("/token")
    //             .header(TTL_TOKEN_HEADER, TTL_TOKEN.as_secs().to_string().as_str());
    //         then.status(200).body(invalid_token);
    //     });
    //
    //     let http_client = AWSHttpClientReqwest::try_new(
    //         client_mock,
    //         imds_server.url("/metadata"),
    //         imds_server.url("/token"),
    //         TTL_TOKEN,
    //     )
    //     .unwrap();
    //
    //     let err = http_client.get().unwrap_err();
    //
    //     assert_matches!(err, HttpClientError::TransportError(_));
    //
    //     token_mock.assert_hits(1);
    // }
}
