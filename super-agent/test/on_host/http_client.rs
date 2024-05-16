use std::net::TcpListener;
use std::{net, thread};

use actix_web::dev::{Server, ServerHandle};
use actix_web::{web, App, HttpRequest, HttpServer, Responder};
use chrono::Utc;
use http::HeaderMap;
use mockall::mock;
use opamp_client::http::http_client::HttpClient;
use tokio::runtime::{Handle, Runtime};
use url::Url;

use newrelic_super_agent::event::channel::{pub_sub, EventConsumer, EventPublisher};
use newrelic_super_agent::opamp::http::auth_token_retriever::{
    TokenRetrieverBuilder, TokenRetrieverBuilderDefault, TokenRetrieverBuilderError,
};
use newrelic_super_agent::opamp::http::builder::{DefaultHttpClientBuilder, HttpClientBuilder};
use newrelic_super_agent::super_agent::config::OpAMPClientConfig;
use nr_auth::token::{AccessToken, Token, TokenType};
use nr_auth::{TokenRetriever, TokenRetrieverError};

use fake::faker::lorem::en::Word;
use fake::Fake;
use tokio::task::JoinHandle;

type Port = u16;

// This test spawns a server that just puts in the response body the content
// of the authorization header received
#[tokio::test(flavor = "multi_thread")]
async fn test_auth_header_is_injected() {
    // Create the server
    let (port, server_handle_cons, join_handle) = run_server().await;
    let server_handle = server_handle_cons.as_ref().recv().unwrap();

    // Create token retriever
    let mut token_retriever_builder = MockTokenRetrieverBuilderMock::default();
    let mut token_retriever = MockTokenRetrieverMock::default();
    let token = token_stub();
    token_retriever.should_retrieve(token.clone());
    token_retriever_builder.should_build(token_retriever);

    // Create http client
    let config = OpAMPClientConfig {
        endpoint: format!("http://127.0.0.1:{}/", port)
            .as_str()
            .try_into()
            .unwrap(),
        headers: HeaderMap::default(),
    };
    let http_client_builder = DefaultHttpClientBuilder::new(config, token_retriever_builder);
    let http_client = http_client_builder.build();

    // Make the post request which must include the token
    let resp = http_client.unwrap().post("".into()).unwrap();

    // Assert that authorization header was present
    let body = std::str::from_utf8(resp.body()).unwrap();
    assert_eq!(format!("Bearer {}", token.access_token()), body);

    // Stop the server
    server_handle.stop(false).await;
    join_handle.await.unwrap();
}

// Run an HTTP Server in a separate tokio thread.
// Return the port, a consumer with the server handle to stop it from outside
// and the join handle to wait.
async fn run_server() -> (Port, EventConsumer<ServerHandle>, JoinHandle<()>) {
    let rt = Handle::current();
    let (server_pub, server_cons) = pub_sub();
    // While binding to port 0, the kernel gives you a free ephemeral port.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = rt.spawn(start_server(server_pub, listener));
    (port, server_cons, handle)
}

// Spawn the HTTP Server
async fn start_server(server_pub: EventPublisher<ServerHandle>, listener: TcpListener) {
    let server: Server = HttpServer::new(|| App::new().service(web::resource("/").to(handler)))
        .listen(listener)
        .expect("Cannot run server")
        .run();
    let _ = server_pub.publish(server.handle());
    let _ = server.await;
}

// A simple handler that will get authorization header and return it as response body
async fn handler(req: HttpRequest) -> impl Responder {
    format!(
        "{}",
        req.headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .ok()
            .unwrap()
    )
}

//////////////////////////////////////////////////////////////////
// Mocks for the Token Retriever. In the Future, it would be nice
// to mock the System Identity Service
//////////////////////////////////////////////////////////////////
mock! {
    pub TokenRetrieverMock {}

    impl TokenRetriever for TokenRetrieverMock{
        fn retrieve(&self) -> Result<Token, TokenRetrieverError>;
    }
}

impl MockTokenRetrieverMock {
    pub fn should_retrieve(&mut self, token: Token) {
        self.expect_retrieve().once().return_once(move || Ok(token));
    }

    pub fn should_return_error(&mut self, error: TokenRetrieverError) {
        self.expect_retrieve()
            .once()
            .return_once(move || Err(error));
    }
}

mock! {
    pub TokenRetrieverBuilderMock {}

    impl TokenRetrieverBuilder for TokenRetrieverBuilderMock{
        type TokenRetriever = MockTokenRetrieverMock;

        fn build(&self) -> Result<<MockTokenRetrieverBuilderMock as TokenRetrieverBuilder>::TokenRetriever, TokenRetrieverBuilderError>;
    }
}

impl MockTokenRetrieverBuilderMock {
    pub fn should_build(&mut self, token_retriever: MockTokenRetrieverMock) {
        self.expect_build()
            .once()
            .return_once(move || Ok(token_retriever));
    }

    pub fn should_fail_on_build(&mut self, error: TokenRetrieverBuilderError) {
        self.expect_build().once().return_once(move || Err(error));
    }
}

pub fn token_stub() -> Token {
    Token::new(
        AccessToken::from(Word().fake::<String>()),
        TokenType::Bearer,
        Utc::now(),
    )
}
