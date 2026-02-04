use super::runtime::tokio_runtime;
use actix_web::{App, HttpResponse, HttpServer, web};
use base64::prelude::BASE64_STANDARD;
use base64::prelude::{BASE64_URL_SAFE_NO_PAD, Engine};
use http::Uri;
use oci_client::Reference;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::json;
use sigstore::cosign::{ClientBuilder, CosignCapabilities, SignatureLayer};
use sigstore::registry::{Auth, ClientConfig, OciReference};
use std::sync::Mutex;
use std::{net, sync::Arc};
use tokio::task::JoinHandle;

const JWKS_SERVER_PATH: &str = "/jwks";
const JWKS_PUBLIC_KEY_ID: &str = "fakeOCIKeyName/0";

/// Represents the state of the OCISigner server.
struct ServerState {
    key_pair: Ed25519KeyPair,
}

/// Fake OCI artifact signer and public key server
pub struct OCISigner {
    handle: JoinHandle<()>,
    state: Arc<Mutex<ServerState>>,
    port: u16,
}

impl OCISigner {
    /// Generates the fake signing key and starts serving the jwks endpoint.
    pub fn start() -> Self {
        // While binding to port 0, the kernel gives you a free ephemeral port.
        let listener = net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let state = ServerState {
            key_pair: Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap(),
        };
        let state = Arc::new(Mutex::new(state));

        let handle = tokio_runtime().spawn(Self::run_http_server(listener, state.clone()));

        Self {
            handle,
            state,
            port,
        }
    }

    /// Returns the url to the jwks path with the public key
    pub fn jwks_url(&self) -> Uri {
        format!("http://localhost:{}{}", self.port, JWKS_SERVER_PATH)
            .parse()
            .unwrap()
    }

    /// Generates and push a cosign singature artifact to the reference.
    pub fn sign_artifact(&self, reference: Reference) {
        tokio_runtime().block_on(async { self.sign_artifact_async(reference).await });
    }

    async fn sign_artifact_async(&self, reference: Reference) {
        let oci_reference: OciReference = reference.to_string().parse().unwrap();

        let client_config = ClientConfig {
            protocol: sigstore::registry::ClientProtocol::Http,
            ..Default::default()
        };
        let mut client = ClientBuilder::default()
            .with_oci_client_config(client_config)
            .build()
            .unwrap();

        // Triangulate to get the cosign signature image reference and source image digest
        let (cosign_signature_image, source_image_digest) = client
            .triangulate(&oci_reference, &Auth::Anonymous)
            .await
            .unwrap();

        // Create unsigned signature layer
        let mut signature_layer =
            SignatureLayer::new_unsigned(&oci_reference, &source_image_digest).unwrap();

        // Sign the payload
        {
            let state = self.state.lock().unwrap();
            let signature = state.key_pair.sign(signature_layer.raw_data.as_ref());
            signature_layer.signature = Some(BASE64_STANDARD.encode(signature.as_ref()));
        }

        // Push the signature to the registry
        client
            .push_signature(
                None,
                &Auth::Anonymous,
                &cosign_signature_image,
                vec![signature_layer],
            )
            .await
            .unwrap();
    }

    async fn run_http_server(listener: net::TcpListener, state: Arc<Mutex<ServerState>>) {
        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(state.clone()))
                .service(web::resource(JWKS_SERVER_PATH).to(jwks_handler))
        })
        .listen(listener)
        .unwrap_or_else(|err| panic!("Could not bind the HTTP server to the listener: {err}"))
        .run()
        .await
        .unwrap_or_else(|err| panic!("Failed to run the HTTP server: {err}"))
    }

    fn stop(&self) {
        self.handle.abort();
    }
}

impl Drop for OCISigner {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn jwks_handler(state: web::Data<Arc<Mutex<ServerState>>>, _req: web::Bytes) -> HttpResponse {
    let server_state = state.lock().unwrap();
    let public_key = server_state.key_pair.public_key().as_ref().to_vec();
    let enc_public_key = BASE64_URL_SAFE_NO_PAD.encode(&public_key);
    let payload = json!({
        "keys": [
            {
                "kty": "OKP",
                "alg": null,
                "use": "sig",
                "kid": JWKS_PUBLIC_KEY_ID,
                "n": null,
                "x": enc_public_key,
                "y": null,
                "crv": "Ed25519"
            }
        ]
    });
    HttpResponse::Ok().json(payload)
}
