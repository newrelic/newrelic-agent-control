//! Compares ways of starting a process inside a cgroup, from a supervisor that may crash.
//!
//! ```text
//! scoping-poc run --backend <b> --name <n> -- <command...>
//! scoping-poc pids --backend <b> --name <n>
//! scoping-poc stop --backend <b> --name <n>
//! scoping-poc probe <file>
//! scoping-poc layout
//! ```
#[cfg(unix)]
mod backend;
#[cfg(unix)]
mod cgroupfs;
#[cfg(unix)]
mod dbus;
#[cfg(unix)]
mod error;
#[cfg(unix)]
mod gated;
#[cfg(unix)]
mod spawn;

#[cfg(unix)]
mod cli;

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    cli::run(std::env::args().skip(1).collect())
}

#[cfg(not(unix))]
fn main() {
    eprintln!("process-scoping-poc only runs on Unix");
}
