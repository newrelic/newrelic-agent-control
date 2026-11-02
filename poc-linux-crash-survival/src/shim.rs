//! Per-sub-agent shim: a tiny long-lived process that is the real parent of the sub-agent, owns
//! its stdout/stderr and answers over a unix socket at a deterministic path. A restarted AC does
//! not search for the sub-agent, it reconnects to its shim.
//!
//! Protocol, one line per request on a fresh connection:
//! `STATUS` -> `RUNNING <pid>` | `EXITED code <n>` | `EXITED signal <n>`; `KILL` -> `OK`;
//! `QUIT` -> `OK` (the shim exits and removes its socket).

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Deterministic socket path for an `(agent_id, exec_id)`, so AC needs no discovery.
pub fn socket_path(dir: &Path, agent_id: &str, exec_id: &str) -> PathBuf {
    dir.join(format!("{agent_id}-{exec_id}.sock"))
}

/// What the shim reports about its sub-agent.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Running {
        pid: u32,
    },
    /// The real exit status, which AC could never get for an orphan it did not spawn.
    ExitedCode(i32),
    ExitedSignal(i32),
}

impl Status {
    fn to_line(&self) -> String {
        match self {
            Status::Running { pid } => format!("RUNNING {pid}"),
            Status::ExitedCode(code) => format!("EXITED code {code}"),
            Status::ExitedSignal(sig) => format!("EXITED signal {sig}"),
        }
    }

    fn from_line(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["RUNNING", pid] => Some(Status::Running {
                pid: pid.parse().ok()?,
            }),
            ["EXITED", "code", n] => Some(Status::ExitedCode(n.parse().ok()?)),
            ["EXITED", "signal", n] => Some(Status::ExitedSignal(n.parse().ok()?)),
            _ => None,
        }
    }
}

fn request(socket: &Path, line: &str) -> io::Result<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    writeln!(stream, "{line}")?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    Ok(reply.trim().to_string())
}

/// Asks the shim at `socket` for its sub-agent's status.
pub fn query(socket: &Path) -> io::Result<Status> {
    let reply = request(socket, "STATUS")?;
    Status::from_line(&reply).ok_or_else(|| io::Error::other(format!("bad reply: {reply:?}")))
}

/// Asks the shim to SIGKILL its sub-agent.
pub fn kill_agent(socket: &Path) -> io::Result<()> {
    request(socket, "KILL").map(|_| ())
}

/// Asks the shim to exit.
pub fn quit(socket: &Path) -> io::Result<()> {
    request(socket, "QUIT").map(|_| ())
}

/// Runs the shim until told to `QUIT`: spawns `program args`, redirecting its stdout and stderr to
/// `log` (a file the shim owns, so nothing breaks when AC dies), and serves `socket`.
///
/// The shim is a subreaper, so descendants that double-fork stay under it instead of going to init.
pub fn run(socket: &Path, log: &Path, program: &str, args: &[String]) -> io::Result<()> {
    nix::sys::prctl::set_child_subreaper(true)?;

    let log_file = OpenOptions::new().create(true).append(true).open(log)?;
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(log_file.try_clone()?)
        .stderr(log_file)
        .spawn()?;
    let child_pid = Pid::from_raw(child.id() as i32);
    // Dropping `Child` neither kills nor waits; the shim reaps it with `waitpid` below.
    drop(child);

    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    listener.set_nonblocking(true)?;

    let mut exited: Option<Status> = None;
    loop {
        reap(child_pid, &mut exited);
        match listener.accept() {
            Ok((stream, _)) => {
                if serve(stream, child_pid, &exited)? {
                    break;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(err),
        }
    }
    let _ = std::fs::remove_file(socket);
    Ok(())
}

/// Reaps every exited child. Records the sub-agent's own exit status; other reaped pids are
/// descendants that were reparented to this subreaper.
fn reap(child_pid: Pid, exited: &mut Option<Status>) {
    loop {
        match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, code)) if pid == child_pid => {
                *exited = Some(Status::ExitedCode(code))
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) if pid == child_pid => {
                *exited = Some(Status::ExitedSignal(sig as i32))
            }
            Ok(WaitStatus::StillAlive) | Err(Errno::ECHILD) => return,
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// Handles one connection. Returns `true` if the shim should exit.
fn serve(stream: UnixStream, child_pid: Pid, exited: &Option<Status>) -> io::Result<bool> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let mut stream = stream;
    match line.trim() {
        "STATUS" => {
            let status = exited.clone().unwrap_or(Status::Running {
                pid: child_pid.as_raw() as u32,
            });
            writeln!(stream, "{}", status.to_line())?;
            Ok(false)
        }
        "KILL" => {
            if exited.is_none() {
                let _ = kill(child_pid, Signal::SIGKILL);
            }
            writeln!(stream, "OK")?;
            Ok(false)
        }
        "QUIT" => {
            writeln!(stream, "OK")?;
            Ok(true)
        }
        other => {
            writeln!(stream, "ERR unknown request {other:?}")?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_round_trip() {
        for status in [
            Status::Running { pid: 42 },
            Status::ExitedCode(7),
            Status::ExitedSignal(9),
        ] {
            assert_eq!(Status::from_line(&status.to_line()), Some(status));
        }
        assert_eq!(Status::from_line("nonsense"), None);
    }
}
