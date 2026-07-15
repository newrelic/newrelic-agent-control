//! Redis lifecycle for the nri-redis E2E scenario on Linux. Uses the `docker` CLI
//! to spin up a Redis 7 container on `localhost:6379`. Cleanup is deterministic
//! via `Drop`.

use crate::common::test::retry_panic;
use std::process::Command;
use std::time::Duration;
use tracing::info;

const CONTAINER_NAME: &str = "ac-e2e-redis";
const IMAGE: &str = "redis:7-alpine";

pub struct Redis;

impl Redis {
    pub fn start() -> Self {
        // Best-effort cleanup of any leftover container from a previous run.
        let _ = Command::new("docker")
            .args(["rm", "-f", CONTAINER_NAME])
            .output();

        info!("Starting Redis container {CONTAINER_NAME}");
        let status = Command::new("docker")
            .args([
                "run", "-d", "--rm",
                "--name", CONTAINER_NAME,
                "-p", "6379:6379",
                IMAGE,
            ])
            .status()
            .expect("failed to spawn docker run for Redis");
        assert!(status.success(), "docker run for Redis failed");

        retry_panic(60, Duration::from_secs(1), "redis PING", || {
            let output = Command::new("docker")
                .args(["exec", CONTAINER_NAME, "redis-cli", "PING"])
                .output()
                .map_err(|e| format!("running redis-cli PING: {e}"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim() == "PONG" {
                Ok(())
            } else {
                Err(format!("PING did not return PONG yet (got: {stdout:?})").into())
            }
        });

        info!("Redis is ready on localhost:6379");
        Self
    }
}

impl Drop for Redis {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", CONTAINER_NAME])
            .output();
    }
}
