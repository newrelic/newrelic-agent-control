use fake::faker::lorem::en::Word;
use fake::Fake;
use http::HeaderMap;
use httpmock::Method::POST;
use httpmock::MockServer;
use opamp_client::http::http_client::HttpClient;
use std::sync::Arc;
use url::Url;

use newrelic_super_agent::opamp::http::builder::{HttpClientBuilder, UreqHttpClientBuilder};
use newrelic_super_agent::super_agent::config::OpAMPClientConfig;
use nr_auth::token_retriever::TokenRetrieverDefault;

// This test spawns a test http server to assert on the received
// authorization headers
#[tokio::test(flavor = "multi_thread")]
async fn test_auth_header_is_injected() {
    // Create the mock server
    let server = MockServer::start();
    // Return a specific response when the header is present so we can assert or response later
    let expected_response = Word().fake::<&str>();
    let _ = server.mock(|when, then| {
        when.method(POST)
            .header("authorization", "Bearer")
            .path("/");
        then.body(expected_response);
    });

    // Create token retriever builder
    let token_retriever = Arc::new(TokenRetrieverDefault::default());

    // Create http client
    let config = OpAMPClientConfig {
        endpoint: Url::parse(server.url("/").to_string().as_str()).unwrap(),
        headers: HeaderMap::default(),
    };
    let http_client_builder = UreqHttpClientBuilder::new(config, token_retriever);
    let http_client = http_client_builder.build();

    // Make the post request which must include the token
    let response = http_client.unwrap().post("".into()).unwrap();
    assert_eq!(
        expected_response,
        std::str::from_utf8(response.body()).unwrap()
    );
}
