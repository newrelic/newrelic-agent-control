use super::runtime::tokio_runtime;
use crate::common::oci::{hex_bytes, push_empty_config_descriptor};
use actix_web::{App, HttpResponse, HttpServer, web};
use base64::prelude::BASE64_STANDARD;
use base64::prelude::{BASE64_URL_SAFE_NO_PAD, Engine};
use http::Uri;
use oci_client::client::{ClientConfig, ClientProtocol::Http};
use oci_client::manifest::{OciDescriptor, OciImageManifest, OciManifest::Image};
use oci_client::secrets::RegistryAuth::Anonymous;
use oci_client::{Client, Reference};
use ring::digest::SHA256;
use ring::digest::digest;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::json;
use std::collections::BTreeMap;
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
    oci_client: Client,
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

        let oci_client = Client::new(ClientConfig {
            protocol: Http,
            ..Default::default()
        });

        Self {
            handle,
            state,
            port,
            oci_client,
        }
    }

    /// Returns the url to the jwks path with the public key
    pub fn jwks_url(&self) -> Uri {
        format!("http://localhost:{}{}", self.port, JWKS_SERVER_PATH)
            .parse()
            .unwrap()
    }

    /// Generates and push a cosign signature artifact to the reference.
    pub fn sign_artifact(&self, reference: Reference) {
        tokio_runtime().block_on(async { self.sign_artifact_async(reference).await });
    }

    async fn sign_artifact_async(&self, reference: Reference) {
        let (cosign_signature_ref, source_image_digest) = self.triangulate(&reference).await;

        let signature_payload =
            self.signature_payload(&source_image_digest, &reference.to_string());

        let signature_layer = self
            .push_signature_layer(&cosign_signature_ref, signature_payload)
            .await;

        let signature_manifest = OciImageManifest {
            schema_version: 2,
            media_type: Some("application/vnd.oci.image.manifest.v1+json".to_string()),
            config: push_empty_config_descriptor(&self.oci_client, &cosign_signature_ref).await,
            layers: vec![signature_layer],
            ..Default::default()
        };

        self.oci_client
            .push_manifest(&cosign_signature_ref, &Image(signature_manifest))
            .await
            .unwrap();
    }

    /// Triangulate the reference to find the digest and build the cosign signature reference.
    async fn triangulate(&self, reference: &Reference) -> (Reference, String) {
        let digest = self
            .oci_client
            .fetch_manifest_digest(reference, &Anonymous)
            .await
            .unwrap();
        let cosign_signature_image = Reference::with_tag(
            reference.registry().to_string(),
            reference.repository().to_string(),
            format!("{}.sig", digest.replace(":", "-")),
        );
        (cosign_signature_image, digest)
    }

    /// Generates the cosign signature payload for the given source digest and reference.
    fn signature_payload(&self, source_digest: &str, source_reference: &str) -> Vec<u8> {
        let payload = json!({
            "critical": {
                "identity": {
                    "docker-reference": source_reference
                },
                "image": {
                    "docker-manifest-digest": source_digest
                },
                "type": "cosign container signature"
            },
            "optional": null
        });
        payload.to_string().into_bytes()
    }

    /// Pushes the signature layer to the registry and returns the corresponding descriptor.
    async fn push_signature_layer(
        &self,
        signature_ref: &Reference,
        signature_payload: Vec<u8>,
    ) -> OciDescriptor {
        let signature_payload_digest = digest(&SHA256, &signature_payload);
        let digest_str = format!("sha256:{}", hex_bytes(signature_payload_digest.as_ref()));

        let encoded_signature;
        {
            let state = self.state.lock().unwrap();
            let signature = state.key_pair.sign(&signature_payload);
            encoded_signature = BASE64_STANDARD.encode(signature.as_ref());
        }

        let signature_layer = OciDescriptor {
            media_type: "application/vnd.dev.cosign.simplesigning.v1+json".to_string(),
            digest: digest_str.clone(),
            size: signature_payload.len() as i64,
            annotations: Some(BTreeMap::from([(
                "dev.cosignproject.cosign/signature".to_string(),
                encoded_signature,
            )])),
            ..Default::default()
        };

        self.oci_client
            .push_blob(signature_ref, signature_payload, &digest_str)
            .await
            .unwrap();

        signature_layer
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
