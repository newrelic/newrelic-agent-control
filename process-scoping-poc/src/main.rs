//! Compares ways of starting a process inside a cgroup, from a supervisor that may crash.
//!
//! ```text
//! scoping-poc run --backend <b> --name <n> -- <command...>
//! scoping-poc pids --backend <b> --name <n>
//! scoping-poc stop --backend <b> --name <n>
//! scoping-poc probe <file>
//! scoping-poc layout
//! ```
#[cfg(target_os = "linux")]
mod backend;
#[cfg(target_os = "linux")]
mod cgroupfs;
#[cfg(target_os = "linux")]
mod dbus;
#[cfg(target_os = "linux")]
mod error;
#[cfg(target_os = "linux")]
mod gated;
#[cfg(target_os = "linux")]
mod spawn;

#[cfg(target_os = "linux")]
mod cli;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    cli::run(std::env::args().skip(1).collect()).await
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("process-scoping-poc only runs on Linux");
}
