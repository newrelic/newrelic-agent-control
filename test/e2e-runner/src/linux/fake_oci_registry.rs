use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

const REPO: &str = "agent-control";
const ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent.v1";
const LAYER_MEDIA_TYPE: &str = "application/vnd.newrelic.agent.content.v1.tar+gzip";

/// An in-process OCI registry that serves a single tar.gz package.
/// Implements just enough of the OCI Distribution Spec for Agent Control to download a package.
pub struct FakeOciRegistry {
    pub addr: String,
}

impl FakeOciRegistry {
    /// Start the registry in a background thread. Returns immediately with the bound address.
    pub fn start(tag: &str, tar_gz: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind fake OCI registry");
        let addr = listener.local_addr().expect("Failed to get local addr").to_string();

        let blob_digest = format!("sha256:{}", sha256_hex(&tar_gz));
        let blob_size = tar_gz.len() as i64;

        let manifest = build_manifest(&blob_digest, blob_size);
        let manifest_bytes = serde_json::to_vec(&manifest).expect("Failed to serialize manifest");
        let manifest_digest = format!("sha256:{}", sha256_hex(&manifest_bytes));

        info!(
            %addr,
            repo = REPO,
            %tag,
            %blob_digest,
            blob_size_bytes = blob_size,
            %manifest_digest,
            "Starting fake OCI registry"
        );
        debug!(
            manifest = %String::from_utf8_lossy(&manifest_bytes),
            "Fake OCI registry manifest"
        );

        let tar_gz = Arc::new(tar_gz);
        let manifest_bytes = Arc::new(manifest_bytes);
        let blob_digest = Arc::new(blob_digest);
        let manifest_digest = Arc::new(manifest_digest);
        let tag = Arc::new(tag.to_string());

        thread::spawn(move || {
            info!("Fake OCI registry accepting connections");
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
                        debug!(%peer, "Fake OCI registry: new connection");
                        let tar_gz = Arc::clone(&tar_gz);
                        let manifest_bytes = Arc::clone(&manifest_bytes);
                        let blob_digest = Arc::clone(&blob_digest);
                        let manifest_digest = Arc::clone(&manifest_digest);
                        let tag = Arc::clone(&tag);
                        thread::spawn(move || {
                            handle_connection(
                                stream,
                                &tar_gz,
                                &manifest_bytes,
                                &blob_digest,
                                &manifest_digest,
                                &tag,
                            );
                        });
                    }
                    Err(e) => {
                        warn!("Fake OCI registry connection error: {e}");
                        break;
                    }
                }
            }
            info!("Fake OCI registry stopped");
        });

        Self { addr }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    tar_gz: &[u8],
    manifest_bytes: &[u8],
    blob_digest: &str,
    manifest_digest: &str,
    tag: &str,
) {
    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };

    let request = String::from_utf8_lossy(&buf[..n]);
    let (method, path) = parse_method_and_path(&request);

    info!(%method, %path, "Fake OCI registry: incoming request");
    debug!(raw_request = %request.lines().next().unwrap_or(""), "Fake OCI registry: request line");

    // GET /v2/ — ping / health check
    if path == "/v2/" || path == "/v2" {
        info!("Fake OCI registry: responding to ping");
        write_response(&mut stream, 200, "application/json", b"{}");
        return;
    }

    // GET or HEAD /v2/{repo}/manifests/{tag or digest}
    let manifest_by_tag = format!("/v2/{REPO}/manifests/{tag}");
    let manifest_by_digest = format!("/v2/{REPO}/manifests/{manifest_digest}");
    if path == manifest_by_tag || path == manifest_by_digest {
        info!(
            %method,
            %path,
            response_bytes = manifest_bytes.len(),
            "Fake OCI registry: serving manifest"
        );
        let content_type = "application/vnd.oci.image.manifest.v1+json";
        if method == "HEAD" {
            write_headers(&mut stream, 200, content_type, manifest_bytes.len());
        } else {
            write_response(&mut stream, 200, content_type, manifest_bytes);
        }
        return;
    }

    // GET or HEAD /v2/{repo}/blobs/{digest}
    let blob_path = format!("/v2/{REPO}/blobs/{blob_digest}");
    if path == blob_path {
        info!(
            %method,
            %path,
            response_bytes = tar_gz.len(),
            "Fake OCI registry: serving blob (tar.gz)"
        );
        if method == "HEAD" {
            write_headers(&mut stream, 200, "application/octet-stream", tar_gz.len());
        } else {
            write_response(&mut stream, 200, "application/octet-stream", tar_gz);
        }
        return;
    }

    warn!(
        %method,
        %path,
        expected_manifest_tag = %manifest_by_tag,
        expected_manifest_digest = %manifest_by_digest,
        expected_blob = %blob_path,
        "Fake OCI registry: 404 — unexpected path"
    );
    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
}

fn write_headers(stream: &mut TcpStream, status: u16, content_type: &str, body_len: usize) {
    let headers = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {body_len}\r\n\r\n"
    );
    let _ = stream.write_all(headers.as_bytes());
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    write_headers(stream, status, content_type, body.len());
    let _ = stream.write_all(body);
}

fn parse_method_and_path(request: &str) -> (String, String) {
    request
        .lines()
        .next()
        .map(|line| {
            let mut parts = line.splitn(3, ' ');
            let method = parts.next().unwrap_or("GET").to_string();
            let path = parts.next().unwrap_or("/").to_string();
            (method, path)
        })
        .unwrap_or_else(|| ("GET".to_string(), "/".to_string()))
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn build_manifest(blob_digest: &str, blob_size: i64) -> serde_json::Value {
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "artifactType": ARTIFACT_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "size": 0
        },
        "layers": [{
            "mediaType": LAYER_MEDIA_TYPE,
            "digest": blob_digest,
            "size": blob_size
        }]
    })
}
