use std::net::TcpListener;

use actix_web::dev::{Server, ServerHandle};
use actix_web::{web, App, HttpRequest, HttpServer, Responder};
use http::HeaderMap;
use opamp_client::http::http_client::HttpClient;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use newrelic_super_agent::event::channel::{pub_sub, EventConsumer, EventPublisher};
use newrelic_super_agent::opamp::http::auth_token_retriever::{
    TokenRetrieverBuilder, TokenRetrieverBuilderDefault,
};
use newrelic_super_agent::opamp::http::builder::{HttpClientBuilder, UreqHttpClientBuilder};
use newrelic_super_agent::super_agent::config::OpAMPClientConfig;

type Port = u16;

// This test spawns a server that just puts in the response body the content
// of the authorization header received
#[tokio::test(flavor = "multi_thread")]
async fn test_auth_header_is_injected() {
    // Create the server
    let (port, server_handle_cons, join_handle) = run_server().await;
    let server_handle = server_handle_cons.as_ref().recv().unwrap();

    // Create token retriever builder
    let token_retriever_builder = TokenRetrieverBuilderDefault;

    // Create http client
    let config = OpAMPClientConfig {
        endpoint: format!("http://127.0.0.1:{}/", port)
            .as_str()
            .try_into()
            .unwrap(),
        headers: HeaderMap::default(),
    };
    let http_client_builder = UreqHttpClientBuilder::new(config, token_retriever_builder);
    let http_client = http_client_builder.build();

    // Make the post request which must include the token
    let resp = http_client.unwrap().post("".into()).unwrap();

    // Assert that authorization header was present.
    let body = std::str::from_utf8(resp.body()).unwrap();
    // Until it is implemented let's assert on the Token being empty
    // so when we implement it this test will fail and we'll fix it
    assert_eq!("Bearer", body);

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
    req.headers()
        .get("authorization")
        .unwrap()
        .to_str()
        .ok()
        .unwrap()
        .to_string()
}
