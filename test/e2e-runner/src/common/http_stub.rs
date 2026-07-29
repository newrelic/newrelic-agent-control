//! Minimal single-endpoint HTTP server used to give the nri-flex E2E scenarios a local target to
//! scrape via its `url` API, without depending on a real third-party service or a web framework.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use tracing::warn;

/// Serves a fixed JSON body on every request until dropped.
pub struct JsonStub {
    port: u16,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl JsonStub {
    /// Binds to an OS-assigned local port and starts serving `body` in a background thread.
    pub fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind http_stub listener");
        let port = listener
            .local_addr()
            .expect("failed to read local_addr")
            .port();
        // Accept must not block forever after `running` is flipped to false, so give it a timeout.
        listener
            .set_nonblocking(false)
            .expect("failed to configure listener");

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if !running_clone.load(Ordering::SeqCst) {
                    break;
                }
                match stream {
                    Ok(stream) => handle_connection(stream, body),
                    Err(e) => warn!("http_stub: accept error: {e}"),
                }
            }
        });

        Self {
            port,
            running,
            handle: Some(handle),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

fn handle_connection(mut stream: TcpStream, body: &str) {
    // We don't care about the request line/headers, just drain them so the client isn't left
    // hanging, then always answer with the same JSON body regardless of path.
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

impl Drop for JsonStub {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        // Unblock the `accept()` loop by connecting to ourselves once.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
