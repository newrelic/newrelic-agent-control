use crate::cloud::http_client::{HttpClient, HttpClientError};
use core::str;
use http::{HeaderMap, HeaderValue, Request, Response};
use std::time::Duration;

const TOKEN_HEADER: &str = "x-aws-ec2-metadata-token";
const TTL_TOKEN_HEADER: &str = "x-aws-ec2-metadata-token-ttl-seconds";

/// An implementation of the `HttpClient` trait.
pub struct AWSHttpClient<C: HttpClient> {
    http_client: C,
    token_endpoint: String,
    token_ttl: Duration,
    metadata_endpoint: String,
}
impl<C: HttpClient> AWSHttpClient<C> {
    /// Returns a new instance of AWSHttpClient
    pub fn new(
        http_client: C,
        metadata_endpoint: String,
        token_endpoint: String,
        token_ttl: Duration,
    ) -> Self {
        Self {
            http_client,
            metadata_endpoint,
            token_endpoint,
            token_ttl,
        }
    }

    fn get_token(&self) -> Result<String, HttpClientError> {
        let mut request = Request::builder()
            .method("PUT")
            .uri(&self.token_endpoint)
            .body(Vec::new())
            .map_err(|e| HttpClientError::BuildingError(e.to_string()))?;
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

        let mut headers = HeaderMap::new();
        headers.insert(
            TOKEN_HEADER,
            HeaderValue::from_str(&token)
                .map_err(|e| HttpClientError::BuildingError(e.to_string()))?,
        );

        self.http_client
            .get(self.metadata_endpoint.clone(), headers)
    }
}

impl<C> HttpClient for AWSHttpClient<C>
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
    use crate::cloud::aws::http_client::AWSHttpClient;
    use crate::cloud::http_client::tests::MockHttpClientMock;
    use crate::cloud::http_client::HttpClientError;
    use http::{Response, StatusCode};
    use mockall::Sequence;
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

        let client = AWSHttpClient::new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        );

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

        let mut seq = Sequence::new();

        mock_http_client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .return_once(move |_| Ok(token_response));

        mock_http_client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .return_once(move |_| Ok(metadata));

        let client = AWSHttpClient::new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        );

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

        let client = AWSHttpClient::new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        );

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

        let mut seq = Sequence::new();

        mock_http_client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .return_once(move |_| Ok(token_response));

        mock_http_client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .return_once(move |_| Ok(metadata));

        let client = AWSHttpClient::new(
            mock_http_client,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN,
        );

        let result = client.get();

        if let Err(HttpClientError::ResponseError(code, message)) = result {
            assert_eq!(code, 403);
            assert_eq!(message, "Forbidden".to_string());
        }
    }
}
