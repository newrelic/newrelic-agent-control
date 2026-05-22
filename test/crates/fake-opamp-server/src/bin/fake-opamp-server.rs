//! Standalone runner for the fake OpAMP server. Intended for manual testing and demos —
//! not for production use.
//!
//! Usage:
//!   fake-opamp-server [BIND_ADDR]
//!
//! BIND_ADDR defaults to `0.0.0.0:0` (a random free port chosen by the OS). The actual
//! address is printed on startup.

use fake_opamp_server::{
    FAKE_SERVER_PATH, FakeServer, JWKS_SERVER_PATH,
    admin::{ADMIN_CONFIG_PATH, ADMIN_STATE_PATH},
};
use std::net::TcpListener;
use std::process::ExitCode;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:0";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bind_addr = match args.as_slice() {
        [] => DEFAULT_BIND_ADDR.to_string(),
        [addr] => addr.clone(),
        _ => {
            eprintln!("usage: fake-opamp-server [BIND_ADDR]");
            eprintln!("  BIND_ADDR defaults to {DEFAULT_BIND_ADDR} (random free port)");
            return ExitCode::from(2);
        }
    };

    let listener = match TcpListener::bind(&bind_addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind on {bind_addr}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let actual_addr = listener.local_addr().expect("local_addr after bind");
    let port = actual_addr.port();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    // Holds the spawned HTTP task alive: dropping `_server` aborts the task.
    let _server = FakeServer::start_with_listener(listener, runtime.handle());

    println!("fake-opamp-server listening on {actual_addr}");
    println!();
    println!("  OpAMP:        POST http://localhost:{port}{FAKE_SERVER_PATH}");
    println!("  JWKS:         GET  http://localhost:{port}{JWKS_SERVER_PATH}");
    println!("  Admin state:  GET  http://localhost:{port}{ADMIN_STATE_PATH}");
    println!("  Admin config: POST http://localhost:{port}{ADMIN_CONFIG_PATH}");
    println!();
    println!("Press Ctrl+C to stop.");

    // Park the main thread on the runtime; Ctrl+C will terminate the process.
    runtime.block_on(std::future::pending::<()>());
    ExitCode::SUCCESS
}
