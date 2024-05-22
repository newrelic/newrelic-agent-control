use http::HeaderMap;
use httptest::{matchers::*, responders::*, Expectation, Server};
use opamp_client::http::http_client::HttpClient;
use url::Url;

use newrelic_super_agent::opamp::http::auth_token_retriever::TokenRetrieverBuilderDefault;
use newrelic_super_agent::opamp::http::builder::{HttpClientBuilder, UreqHttpClientBuilder};
use newrelic_super_agent::super_agent::config::OpAMPClientConfig;

type Port = u16;

// This test spawns a test http server to assert on the received
// authorization headers
#[tokio::test(flavor = "multi_thread")]
async fn test_auth_header_is_injected() {
    // Create the server with expectation
    let server = Server::run();
    server.expect(
        Expectation::matching(all_of![
            request::headers(contains(("authorization", "Bearer"))),
            request::method("POST"),
            request::path("/"),
        ])
        .respond_with(status_code(200)),
    );

    // Create token retriever builder
    let token_retriever_builder = TokenRetrieverBuilderDefault;

    // Create http client
    let config = OpAMPClientConfig {
        endpoint: Url::parse(server.url("/").to_string().as_str()).unwrap(),
        headers: HeaderMap::default(),
    };
    let http_client_builder = UreqHttpClientBuilder::new(config, token_retriever_builder);
    let http_client = http_client_builder.build();

    // Make the post request which must include the token
    http_client.unwrap().post("".into()).unwrap();
}
