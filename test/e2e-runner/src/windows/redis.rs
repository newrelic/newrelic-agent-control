//! Redis lifecycle for the nri-redis E2E scenario on Windows. Downloads a
//! self-contained Redis-x64 5.x zip from tporadowski/redis, extracts it, and
//! runs `redis-server.exe` on `localhost:6379`. Cleanup is deterministic via
//! [LongRunningProcess]'s Drop.

use crate::common::exec::LongRunningProcess;
use crate::common::test::retry_panic;
use crate::windows::powershell::{download_file, extract};
use crate::windows::utils::as_user_dir;
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;
use tracing::info;

const REDIS_VERSION: &str = "5.0.14.1";
const REDIS_URL: &str =
    "https://github.com/tporadowski/redis/releases/download/v5.0.14.1/Redis-x64-5.0.14.1.zip";
const REDIS_ZIP: &str = "\\redis.zip";
const REDIS_DIR: &str = "\\redis";
const REDIS_BIN: &str = "\\redis\\redis-server.exe";

pub struct Redis {
    _process: LongRunningProcess,
}

impl Redis {
    pub fn start() -> Self {
        let zip_path = as_user_dir(REDIS_ZIP);
        let extract_path = as_user_dir(REDIS_DIR);
        let bin_path = as_user_dir(REDIS_BIN);

        info!(url = REDIS_URL, version = REDIS_VERSION, "Downloading Redis for Windows");
        download_file(REDIS_URL, &zip_path);
        extract(&zip_path, &extract_path);

        info!("Spawning redis-server on localhost:6379");
        let mut cmd = Command::new(&bin_path);
        cmd.args(["--port", "6379", "--protected-mode", "no"]);
        let process = LongRunningProcess::spawn(cmd);

        retry_panic(60, Duration::from_secs(1), "redis TCP connect", || {
            TcpStream::connect_timeout(
                &"127.0.0.1:6379".parse().unwrap(),
                Duration::from_secs(1),
            )
            .map(|_| ())
            .map_err(|e| format!("TCP connect to Redis: {e}").into())
        });

        info!("Redis is ready on localhost:6379");
        Self { _process: process }
    }
}
