//! Backend C: the child is held in `pre_exec` until systemd has adopted its PID into a transient
//! scope, so the target program starts already inside the scope.
//!
//! `Command::spawn` does not return until the child has executed the program, so it runs on a
//! blocking thread while the PID reaches this task over a pipe.
use crate::backend::{Started, Waiter};
use crate::dbus::Systemd;
use crate::error::{Error, Result, ctx};
use crate::spawn::command;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::time::Duration;

fn pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as RawFd; 2];
    // SAFETY: `pipe2` fills the array with two new descriptors that we take ownership of.
    unsafe {
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return Err(Error::new(format!(
                "creating pipe: {}",
                io::Error::last_os_error()
            )));
        }
        Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])))
    }
}

pub async fn start_scope(unit: &str, argv: &[String]) -> Result<Started> {
    let (pid_r, pid_w) = pipe()?;
    let (gate_r, gate_w) = pipe()?;
    let (pid_r_raw, pid_w_raw, gate_r_raw, gate_w_raw) = {
        use std::os::fd::AsRawFd;
        (
            pid_r.as_raw_fd(),
            pid_w.as_raw_fd(),
            gate_r.as_raw_fd(),
            gate_w.as_raw_fd(),
        )
    };

    let mut cmd = command(argv)?;
    // SAFETY: the closure only calls async-signal-safe functions and does not allocate.
    unsafe {
        cmd.pre_exec(move || {
            libc::close(pid_r_raw);
            libc::close(gate_w_raw);
            let pid = libc::getpid().to_ne_bytes();
            if libc::write(pid_w_raw, pid.as_ptr().cast(), pid.len()) != pid.len() as isize {
                return Err(io::Error::last_os_error());
            }
            let mut byte = 0u8;
            loop {
                match libc::read(gate_r_raw, (&mut byte as *mut u8).cast(), 1) {
                    1 => return Ok(()),
                    0 => return Err(io::ErrorKind::UnexpectedEof.into()),
                    _ => {
                        let err = io::Error::last_os_error();
                        if err.kind() != io::ErrorKind::Interrupted {
                            return Err(err);
                        }
                    }
                }
            }
        });
    }

    let spawner = tokio::task::spawn_blocking(move || cmd.spawn());
    let pid = tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4];
        File::from(pid_r)
            .read_exact(&mut buf)
            .map(|()| u32::from_ne_bytes(buf))
    })
    .await
    .map_err(ctx("joining pid task"))?
    .map_err(ctx("reading child pid"))?;

    let adopted = adopt(unit, pid).await;
    if adopted.is_ok() {
        File::from(gate_w)
            .write_all(&[1])
            .map_err(ctx("releasing child"))?;
    } else {
        // Closing the gate makes the pending child fail instead of running outside the scope.
        drop(gate_w);
    }
    let child = spawner
        .await
        .map_err(ctx("joining spawn task"))?
        .map_err(ctx("spawning process"));
    drop((pid_w, gate_r));
    adopted?;
    Ok(Started {
        pid,
        waiter: Waiter::Child(child?),
    })
}

async fn adopt(unit: &str, pid: u32) -> Result<()> {
    Systemd::connect().await?.start_scope(unit, pid).await?;
    for _ in 0..50 {
        let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
        if cgroup.contains(unit) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(Error::new(format!("pid {pid} did not join {unit}")))
}
