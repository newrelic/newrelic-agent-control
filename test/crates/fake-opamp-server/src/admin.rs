//! Admin-facing types and HTTP handlers for the fake OpAMP server.
//!
//! These are intended for human inspection and manual driving of the server in tests/demos,
//! not for any stable wire format. Proto-typed fields are rendered via their `Debug` impl.

use crate::{AgentState, JWKS_PUBLIC_KEY_ID, RemoteConfig, ServerState};
use actix_web::{HttpResponse, web};
use opamp_client::operation::instance_uid::InstanceUid;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Path for the admin endpoint that returns a JSON snapshot of the server state.
pub const ADMIN_STATE_PATH: &str = "/admin/state";

/// Path template for the admin endpoint that sets the pending remote config for a given agent.
/// `{instance_uid}` accepts the canonical UUIDv7 string (uppercase, no hyphens, as produced by
/// `InstanceUid::Display`); both hyphenated and unhyphenated forms are valid.
pub const ADMIN_CONFIG_PATH: &str = "/admin/agents/{instance_uid}/config";

/// Human-readable, JSON-friendly snapshot of the server state.
#[derive(Serialize)]
pub struct ServerStateView {
    pub jwks_public_key_id: String,
    /// Connected agents keyed by `InstanceUid` in its canonical (uppercase, no-hyphen) form.
    pub agents: HashMap<String, AgentStateView>,
}

#[derive(Serialize)]
pub struct AgentStateView {
    pub sequence_number: u64,
    pub health_status: Option<String>,
    pub attributes: String,
    pub effective_config: String,
    pub config_status: String,
    pub pending_remote_config: Option<RemoteConfigView>,
}

#[derive(Serialize)]
pub struct RemoteConfigView {
    pub config_hash: String,
    pub config_map: HashMap<String, String>,
}

impl From<&ServerState> for ServerStateView {
    fn from(state: &ServerState) -> Self {
        Self {
            jwks_public_key_id: JWKS_PUBLIC_KEY_ID.to_string(),
            agents: state
                .agent_state
                .iter()
                .map(|(uid, agent)| (uid.to_string(), agent.into()))
                .collect(),
        }
    }
}

impl From<&AgentState> for AgentStateView {
    fn from(s: &AgentState) -> Self {
        Self {
            sequence_number: s.sequence_number,
            health_status: s.health_status.as_ref().map(|h| format!("{h:?}")),
            attributes: format!("{:?}", s.attributes),
            effective_config: format!("{:?}", s.effective_config),
            config_status: format!("{:?}", s.config_status),
            pending_remote_config: s.remote_config.as_ref().map(Into::into),
        }
    }
}

impl From<&RemoteConfig> for RemoteConfigView {
    fn from(rc: &RemoteConfig) -> Self {
        let config_hash = String::from_utf8_lossy(&rc.0.config_hash).to_string();
        let config_map =
            rc.0.config
                .as_ref()
                .map(|cm| {
                    cm.config_map
                        .iter()
                        .map(|(k, v)| (k.clone(), String::from_utf8_lossy(&v.body).to_string()))
                        .collect()
                })
                .unwrap_or_default();
        Self {
            config_hash,
            config_map,
        }
    }
}

/// Returns a JSON snapshot of the current server state.
pub(crate) async fn get_state_handler(state: web::Data<Arc<Mutex<ServerState>>>) -> HttpResponse {
    let state = state.lock().unwrap();
    HttpResponse::Ok().json(ServerStateView::from(&*state))
}

/// Sets the pending remote config for the agent identified by the path parameter, overwriting
/// any previous config. The body is the raw `key -> yaml-body` map.
pub(crate) async fn set_config_handler(
    state: web::Data<Arc<Mutex<ServerState>>>,
    path: web::Path<String>,
    body: web::Json<HashMap<String, String>>,
) -> HttpResponse {
    let instance_uid = match InstanceUid::try_from(path.into_inner()) {
        Ok(uid) => uid,
        Err(e) => return HttpResponse::BadRequest().body(format!("invalid instance_uid: {e}")),
    };
    state
        .lock()
        .unwrap()
        .set_multi_config(instance_uid, body.into_inner());
    HttpResponse::NoContent().finish()
}
