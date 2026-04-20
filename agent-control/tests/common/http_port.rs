use std::net::TcpListener;

/// Returns an available local port by binding and immediately releasing an ephemeral socket.
pub fn available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind ephemeral port")
        .local_addr()
        .unwrap()
        .port()
}
