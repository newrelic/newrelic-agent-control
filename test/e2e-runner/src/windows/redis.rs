//! Redis lifecycle for the nri-redis E2E scenario on Windows. Installs
//! Memurai Developer (Redis-compatible, Windows-native) via Chocolatey and
//! ensures its Windows service is running on `localhost:6379`. Cleanup is
//! deterministic via [Redis]'s Drop: the service is stopped.

use crate::common::test::retry_panic;
use crate::windows::powershell::exec_ps;
use crate::windows::service::{STATUS_RUNNING, check_service_status};
use std::net::TcpStream;
use std::time::Duration;
use tracing::info;

const CHOCO_PACKAGE: &str = "memurai-developer";
const MEMURAI_SERVICE: &str = "Memurai";

pub struct Redis;

impl Redis {
    pub fn start() -> Self {
        info!(
            package = CHOCO_PACKAGE,
            "Installing Memurai Developer via Chocolatey"
        );
        exec_ps(format!(
            "choco install {CHOCO_PACKAGE} -y --no-progress --limit-output"
        ))
        .unwrap_or_else(|err| panic!("Failed to install {CHOCO_PACKAGE} via Chocolatey: {err}"));

        info!(
            service = MEMURAI_SERVICE,
            "Ensuring Memurai service is running"
        );
        exec_ps(format!("Start-Service -Name '{MEMURAI_SERVICE}'"))
            .unwrap_or_else(|err| panic!("Failed to start '{MEMURAI_SERVICE}' service: {err}"));

        retry_panic(
            30,
            Duration::from_secs(1),
            "Memurai service running",
            || check_service_status(MEMURAI_SERVICE, STATUS_RUNNING),
        );

        retry_panic(60, Duration::from_secs(1), "redis TCP connect", || {
            TcpStream::connect_timeout(&"127.0.0.1:6379".parse().unwrap(), Duration::from_secs(1))
                .map(|_| ())
                .map_err(|e| format!("TCP connect to Redis: {e}").into())
        });

        info!("Memurai is ready on localhost:6379");
        Self
    }
}

impl Drop for Redis {
    fn drop(&mut self) {
        info!(service = MEMURAI_SERVICE, "Stopping Memurai service");
        // Swallow errors: teardown must not double-panic during unwinding.
        let _ = exec_ps(format!("Stop-Service -Name '{MEMURAI_SERVICE}' -Force"));
    }
}
