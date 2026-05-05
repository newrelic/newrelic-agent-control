//! HTTP handlers for diagnostics endpoints

use actix_web::{HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use super::metrics::{DiagnosticsSnapshot, format_bytes};
use super::spawn_tracker::TaskInfo;
use super::{ProfilingState, SpawnTracker};

/// Shared state for diagnostics handlers
#[derive(Clone)]
pub struct DiagnosticsState {
    pub spawn_tracker: Option<Arc<SpawnTracker>>,
    pub profiling: Arc<ProfilingState>,
}

/// Request to start CPU profiling
#[derive(Debug, Deserialize)]
pub struct StartProfilingRequest {
    /// Sampling frequency in Hz (default: 100)
    #[serde(default = "default_frequency")]
    pub frequency: i32,
    /// Optional session name
    pub session_name: Option<String>,
}

fn default_frequency() -> i32 {
    100
}

/// Request to stop profiling
#[derive(Debug, Deserialize)]
pub struct StopProfilingRequest {
    /// Output format: "flamegraph" or "pprof"
    #[serde(default = "default_format")]
    pub format: String,
    /// Output path (optional, will generate default if not provided)
    pub output_path: Option<String>,
}

fn default_format() -> String {
    "flamegraph".to_string()
}

/// Response for profiling operations
#[derive(Debug, Serialize)]
pub struct ProfilingResponse {
    pub success: bool,
    pub message: String,
    pub output_path: Option<String>,
}

/// Response for spawn tracking queries
#[derive(Debug, Serialize)]
pub struct SpawnTrackingResponse {
    pub active_tasks: Vec<TaskInfo>,
    pub total_active: usize,
    pub stats: Option<crate::diagnostics::spawn_tracker::SpawnStats>,
}

/// Response for leak detection
#[derive(Debug, Serialize)]
pub struct LeakDetectionResponse {
    pub potential_leaks: Vec<TaskInfo>,
    pub count: usize,
    pub threshold_secs: f64,
}

/// GET /diagnostics - Get overall diagnostics snapshot
pub async fn get_diagnostics(state: web::Data<DiagnosticsState>) -> impl Responder {
    let snapshot = DiagnosticsSnapshot::capture(state.spawn_tracker.as_ref().map(|t| t.as_ref()));

    HttpResponse::Ok().json(snapshot)
}

/// GET /diagnostics/spawns/active - Get active spawns
pub async fn get_active_spawns(state: web::Data<DiagnosticsState>) -> impl Responder {
    match &state.spawn_tracker {
        Some(tracker) => {
            let active_tasks = tracker.active_tasks();
            let stats = tracker.stats();

            HttpResponse::Ok().json(SpawnTrackingResponse {
                total_active: active_tasks.len(),
                active_tasks,
                stats: Some(stats),
            })
        }
        None => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "Spawn tracking not enabled"
        })),
    }
}

/// GET /diagnostics/spawns/completed?limit=100 - Get completed spawns
pub async fn get_completed_spawns(
    state: web::Data<DiagnosticsState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    match &state.spawn_tracker {
        Some(tracker) => {
            let limit = query
                .get("limit")
                .and_then(|s| s.parse().ok())
                .unwrap_or(100);

            let completed = tracker.completed_tasks(limit);

            HttpResponse::Ok().json(serde_json::json!({
                "completed_tasks": completed,
                "count": completed.len()
            }))
        }
        None => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "Spawn tracking not enabled"
        })),
    }
}

/// GET /diagnostics/spawns/leaks?threshold_secs=300 - Detect potential leaks
pub async fn detect_leaks(
    state: web::Data<DiagnosticsState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    match &state.spawn_tracker {
        Some(tracker) => {
            let threshold_secs = query
                .get("threshold_secs")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(300.0);

            let threshold = Duration::from_secs_f64(threshold_secs);
            let leaks = tracker.potential_leaks(threshold);

            HttpResponse::Ok().json(LeakDetectionResponse {
                count: leaks.len(),
                potential_leaks: leaks,
                threshold_secs,
            })
        }
        None => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "Spawn tracking not enabled"
        })),
    }
}

/// POST /diagnostics/profiling/start - Start CPU profiling
pub async fn start_profiling(
    state: web::Data<DiagnosticsState>,
    req: web::Json<StartProfilingRequest>,
) -> impl Responder {
    match state
        .profiling
        .start(req.frequency, req.session_name.clone())
    {
        Ok(()) => HttpResponse::Ok().json(ProfilingResponse {
            success: true,
            message: format!("Profiling started with frequency {} Hz", req.frequency),
            output_path: None,
        }),
        Err(e) => HttpResponse::BadRequest().json(ProfilingResponse {
            success: false,
            message: e.to_string(),
            output_path: None,
        }),
    }
}

/// POST /diagnostics/profiling/stop - Stop CPU profiling and generate output
pub async fn stop_profiling(
    state: web::Data<DiagnosticsState>,
    req: web::Json<StopProfilingRequest>,
) -> impl Responder {
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");

    let output_path = req.output_path.clone().unwrap_or_else(|| {
        if req.format == "pprof" {
            format!("/tmp/profile_{}.pb", timestamp)
        } else {
            format!("/tmp/flamegraph_{}.svg", timestamp)
        }
    });

    let result = if req.format == "pprof" {
        state.profiling.stop_pprof(&output_path)
    } else {
        state.profiling.stop_flamegraph(&output_path)
    };

    match result {
        Ok(path) => HttpResponse::Ok().json(ProfilingResponse {
            success: true,
            message: format!("Profiling stopped. Output saved to {}", path),
            output_path: Some(path),
        }),
        Err(e) => HttpResponse::BadRequest().json(ProfilingResponse {
            success: false,
            message: e.to_string(),
            output_path: None,
        }),
    }
}

/// GET /diagnostics/profiling/status - Get profiling status
pub async fn profiling_status(state: web::Data<DiagnosticsState>) -> impl Responder {
    let status = state.profiling.status();
    HttpResponse::Ok().json(status)
}

/// GET /diagnostics/health - Simple health check with diagnostics
pub async fn health_check(state: web::Data<DiagnosticsState>) -> impl Responder {
    let runtime_metrics = crate::diagnostics::metrics::get_runtime_metrics();
    let memory_metrics = crate::diagnostics::metrics::get_memory_metrics();

    let spawn_info = state.spawn_tracker.as_ref().map(|tracker| {
        let stats = tracker.stats();
        serde_json::json!({
            "active": stats.currently_active,
            "total_spawned": stats.total_spawned,
            "total_completed": stats.total_completed
        })
    });

    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "workers": runtime_metrics.num_workers,
        "spawns": spawn_info,
        "memory": {
            "physical": memory_metrics.physical_mem_bytes.map(format_bytes),
            "virtual": memory_metrics.virtual_mem_bytes.map(format_bytes)
        }
    }))
}

/// DELETE /diagnostics/spawns/history - Clear completed spawn history
pub async fn clear_spawn_history(state: web::Data<DiagnosticsState>) -> impl Responder {
    match &state.spawn_tracker {
        Some(tracker) => {
            tracker.clear_history();
            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "Spawn history cleared"
            }))
        }
        None => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "Spawn tracking not enabled"
        })),
    }
}

/// Configure diagnostics routes
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/diagnostics").route(web::get().to(get_diagnostics)))
        .service(web::resource("/diagnostics/health").route(web::get().to(health_check)))
        .service(
            web::resource("/diagnostics/spawns/active").route(web::get().to(get_active_spawns)),
        )
        .service(
            web::resource("/diagnostics/spawns/completed")
                .route(web::get().to(get_completed_spawns)),
        )
        .service(web::resource("/diagnostics/spawns/leaks").route(web::get().to(detect_leaks)))
        .service(
            web::resource("/diagnostics/spawns/history")
                .route(web::delete().to(clear_spawn_history)),
        )
        .service(
            web::resource("/diagnostics/profiling/start").route(web::post().to(start_profiling)),
        )
        .service(web::resource("/diagnostics/profiling/stop").route(web::post().to(stop_profiling)))
        .service(
            web::resource("/diagnostics/profiling/status").route(web::get().to(profiling_status)),
        );
}
