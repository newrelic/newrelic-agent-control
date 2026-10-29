//! Fleet Control load-test simulator: runs many on-host Agent Controls in one process, each
//! looking like a separate host, with no-op sub-agents. See `README.md` next to this file.
mod config;
mod layout;

use config::{FakeAgentTypeId, LoadTestConfig};
use layout::{AcLayout, FakeAgentType};
use newrelic_agent_control::agent_control::config::AgentControlConfig;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_control::run::{
    AgentControlRunner, BasePaths, LoadTestOverrides, SharedRunnerResources,
    probe_dynamic_custom_capabilities,
};
use newrelic_agent_control::agent_type::registry::embedded_agent_type_ids;
use newrelic_agent_control::command::RunnerContext;
use newrelic_agent_control::event::ApplicationEvent;
use newrelic_agent_control::event::channel::{EventPublisher, pub_sub};
use newrelic_agent_control::http::client::HttpClient;
use newrelic_agent_control::http::config::{HttpConfig, ProxyConfig};
use newrelic_agent_control::oci;
use newrelic_agent_control::opamp::auth::token_retriever::TokenRetrieverImpl;
use newrelic_agent_control::opamp::http::builder::SharedOpAMPHttp;
use newrelic_agent_control::opamp::secret_retriever::on_host::retrieve::OnHostSecretRetriever;
use newrelic_agent_control::value_provider::file::FileProvider;
use std::error::Error;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::{debug, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

const HTTP_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
const RAMP_UP_POLL: Duration = Duration::from_millis(100);
/// `kern.num_taskthreads` default on macOS.
const MACOS_THREADS_PER_PROCESS: usize = 16_384;
/// Measured locally: ~18 per Agent Control (AC loop, OpAMP poller, HTTP client threads, plus
/// event loop, OpAMP poller and health checker for each of 4 sub-agents).
const ESTIMATED_THREADS_PER_AC: usize = 18;

fn main() -> ExitCode {
    let Some(config_path) = std::env::args().nth(1) else {
        eprintln!("usage: newrelic-agent-control-fc-load-test <config.yaml>");
        return ExitCode::FAILURE;
    };
    let config = match LoadTestConfig::load(Path::new(&config_path)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_level))
        .init();

    match run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("Load test failed: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(config: LoadTestConfig) -> Result<(), Box<dyn Error>> {
    let template = config.template()?;
    let fleet_id = template
        .fleet_control
        .as_ref()
        .map(|fc| fc.fleet_id.clone())
        .unwrap_or_default();

    if config.fresh {
        info!(run_prefix = %config.run_prefix, "Wiping previous data of the run prefix");
        layout::wipe_range(&config.data_root, &config.run_prefix, indices(&config))?;
    }
    let healthy_file = layout::write_healthy_file(&config.data_root)?;
    let fake_agent_types = fake_agent_types(&config, &healthy_file);

    if exceeds_thread_budget(config.count) {
        warn!(
            count = config.count,
            estimated_threads = estimated_threads(config.count),
            limit = MACOS_THREADS_PER_PROCESS,
            "Expected thread count is close to the macOS per-process limit, split the load with index_offset"
        );
    }

    let shared = shared_runner_resources(&template)?;
    let token_retriever = shared_token_retriever(&template, &config.data_root)?;

    let stopping = Arc::new(AtomicBool::new(false));
    let stop_publishers: Arc<Mutex<Vec<EventPublisher<ApplicationEvent>>>> = Arc::default();
    {
        let stopping = stopping.clone();
        let stop_publishers = stop_publishers.clone();
        ctrlc::set_handler(move || {
            info!("Stop requested, stopping every simulated Agent Control");
            stopping.store(true, Ordering::SeqCst);
            for publisher in stop_publishers.lock().unwrap().iter() {
                let _ = publisher.publish(ApplicationEvent::StopRequested);
            }
        })?;
    }

    let spawn_interval = Duration::from_secs(1) / config.ramp_up_per_second;
    let (handles, ramp_up_error) = ramp_up(indices(&config), spawn_interval, &stopping, |index| {
        let ac_layout = AcLayout::new(&config.data_root, &config.run_prefix, index);
        layout::prepare(&ac_layout, &config.agent_control, &fake_agent_types)?;

        let (publisher, consumer) = pub_sub();
        stop_publishers.lock().unwrap().push(publisher.clone());
        // The handler may have run between the check in `ramp_up` and the registration.
        if stopping.load(Ordering::SeqCst) {
            let _ = publisher.publish(ApplicationEvent::StopRequested);
        }

        let context = RunnerContext {
            bootstrap_config: read_local_config(&ac_layout.local_config_path())?,
            base_paths: BasePaths {
                local_dir: ac_layout.local_dir.clone(),
                remote_dir: ac_layout.remote_dir.clone(),
                log_dir: ac_layout.log_dir.clone(),
            },
            running_mode: AGENT_CONTROL_MODE_ON_HOST,
            application_event_consumer: consumer,
            self_replace_target: None,
        };
        let overrides = LoadTestOverrides {
            identifiers: layout::identifiers(&ac_layout.name, &fleet_id),
            shared_http: Some(per_ac_opamp_http(&token_retriever, &template.proxy)?),
        };
        Ok(spawn_simulated_ac(
            ac_layout.name,
            context,
            shared.clone(),
            overrides,
        )?)
    });

    if let Some(err) = &ramp_up_error {
        error!("Ramp-up failed, stopping every simulated Agent Control: {err}");
        stopping.store(true, Ordering::SeqCst);
        for publisher in stop_publishers.lock().unwrap().iter() {
            let _ = publisher.publish(ApplicationEvent::StopRequested);
        }
    }

    match process_thread_count() {
        Some(threads) => info!(started = handles.len(), threads, "Ramp-up complete"),
        None => info!(started = handles.len(), "Ramp-up complete"),
    }

    for handle in handles {
        let _ = handle.join();
    }
    info!("Every simulated Agent Control stopped");
    ramp_up_error.map_or(Ok(()), Err)
}

fn indices(config: &LoadTestConfig) -> Range<usize> {
    config.index_offset..config.index_offset + config.count
}

/// Spawns one simulated Agent Control per index, pacing them by `interval`. Stops at the first
/// error or stop request and returns the handles spawned so far with the error, if any.
fn ramp_up<F>(
    indices: Range<usize>,
    interval: Duration,
    stopping: &AtomicBool,
    mut spawn: F,
) -> (Vec<JoinHandle<()>>, Option<Box<dyn Error>>)
where
    F: FnMut(usize) -> Result<JoinHandle<()>, Box<dyn Error>>,
{
    let mut handles = Vec::with_capacity(indices.len());
    for index in indices {
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        match spawn(index) {
            Ok(handle) => handles.push(handle),
            Err(err) => return (handles, Some(err)),
        }
        sleep_unless_stopping(interval, stopping);
    }
    (handles, None)
}

/// Rough thread count of `count` simulated Agent Controls with 4 configured sub-agents each.
fn estimated_threads(count: usize) -> usize {
    count * ESTIMATED_THREADS_PER_AC
}

/// Whether the estimate goes over 90% of the macOS per-process thread limit.
fn exceeds_thread_budget(count: usize) -> bool {
    estimated_threads(count) * 10 > MACOS_THREADS_PER_PROCESS * 9
}

/// No-op agent types shadowing every embedded agent type plus the configured extra ones.
fn fake_agent_types(config: &LoadTestConfig, healthy_file: &Path) -> Vec<FakeAgentType> {
    let mut ids: Vec<FakeAgentTypeId> = embedded_agent_type_ids(AGENT_CONTROL_MODE_ON_HOST)
        .into_iter()
        .map(|id| FakeAgentTypeId {
            namespace: id.namespace().to_string(),
            name: id.name().to_string(),
            version: id.version().to_string(),
        })
        .collect();
    for extra in &config.fake_agent_types {
        if !ids.contains(extra) {
            ids.push(extra.clone());
        }
    }
    info!(count = ids.len(), "Shadowing agent types with no-op ones");
    ids.iter()
        .inspect(|id| debug!(namespace = %id.namespace, name = %id.name, version = %id.version, "Shadowed agent type"))
        .map(|id| layout::fake_agent_type(id, healthy_file, config.health_interval))
        .collect()
}

/// Runtime, OCI client and probe result shared by every simulated Agent Control. The probe runs
/// against the template's registry, not the unreachable one forced into each local config.
fn shared_runner_resources(
    template: &AgentControlConfig,
) -> Result<SharedRunnerResources, Box<dyn Error>> {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?,
    );
    let oci_client =
        oci::Client::try_new(Default::default(), template.proxy.clone(), runtime.clone())?;
    let dynamic_custom_capabilities = probe_dynamic_custom_capabilities(&oci_client, template);
    Ok(SharedRunnerResources {
        runtime,
        oci_client,
        dynamic_custom_capabilities,
    })
}

/// Token retriever shared by every OpAMP client in the process. No-op without `auth_config`.
fn shared_token_retriever(
    template: &AgentControlConfig,
    data_root: &Path,
) -> Result<Arc<TokenRetrieverImpl>, Box<dyn Error>> {
    let fleet_control = template.fleet_control.clone();
    let auth_config = fleet_control.as_ref().and_then(|fc| fc.auth_config.clone());
    if auth_config.is_none() {
        warn!("No auth_config in the template, OpAMP requests are not authenticated");
    }
    let secret_retriever =
        OnHostSecretRetriever::new(fleet_control, data_root.to_path_buf(), FileProvider);
    let token_retriever =
        TokenRetrieverImpl::try_build(auth_config, &secret_retriever, template.proxy.clone())?;
    Ok(Arc::new(token_retriever))
}

/// HTTP client shared by the OpAMP clients of one simulated Agent Control, so every simulated
/// host opens its own connections, plus the process-wide token retriever.
fn per_ac_opamp_http(
    token_retriever: &Arc<TokenRetrieverImpl>,
    proxy: &ProxyConfig,
) -> Result<SharedOpAMPHttp, Box<dyn Error>> {
    let http_client = HttpClient::new(HttpConfig::new(
        HTTP_CLIENT_TIMEOUT,
        HTTP_CLIENT_TIMEOUT,
        proxy.clone(),
    ))?;
    Ok(SharedOpAMPHttp {
        http_client,
        token_retriever: token_retriever.clone(),
    })
}

fn read_local_config(path: &PathBuf) -> Result<AgentControlConfig, Box<dyn Error>> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_saphyr::from_str(&content)?)
}

fn spawn_simulated_ac(
    name: String,
    context: RunnerContext,
    shared: SharedRunnerResources,
    overrides: LoadTestOverrides,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            let span = info_span!("simulated_ac", ac = %name);
            let _guard = span.enter();
            let result = AgentControlRunner::try_new_with_shared(context, shared, overrides)
                .and_then(|runner| runner.run_onhost().map_err(|err| err.into()));
            if let Err(err) = result {
                error!(ac = %name, "Simulated Agent Control failed: {err}");
            }
        })
}

fn sleep_unless_stopping(duration: Duration, stopping: &AtomicBool) {
    let mut remaining = duration;
    while !remaining.is_zero() && !stopping.load(Ordering::SeqCst) {
        let step = remaining.min(RAMP_UP_POLL);
        std::thread::sleep(step);
        remaining -= step;
    }
}

#[cfg(target_os = "macos")]
fn process_thread_count() -> Option<usize> {
    let output = std::process::Command::new("ps")
        .args(["-M", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let lines = String::from_utf8_lossy(&output.stdout).lines().count();
    lines.checked_sub(1)
}

#[cfg(target_os = "linux")]
fn process_thread_count() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .and_then(|value| value.trim().parse().ok())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_thread_count() -> Option<usize> {
    None
}
