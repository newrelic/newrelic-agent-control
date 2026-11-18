//! Backend C: the child is held in `pre_exec` until systemd has adopted its PID into a transient
//! scope, so the target program starts already inside the scope.
//!
//! `Command::spawn` does not return until the child has executed the program, so it runs on its
//! own thread while the PID reaches this one over a pipe.
use crate::backend::{Started, Waiter};
use crate::dbus;
use crate::error::{Error, Result, ctx};
use crate::spawn::command;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use nix::unistd;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::thread;
use std::time::Duration;

/// A pipe whose ends do not survive `exec`.
fn pipe() -> Result<(OwnedFd, OwnedFd)> {
    let (read, write) = unistd::pipe().map_err(ctx("creating pipe"))?;
    for fd in [&read, &write] {
        fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)).map_err(ctx("setting close-on-exec"))?;
    }
    Ok((read, write))
}

pub fn start_scope(unit: &str, argv: &[String]) -> Result<Started> {
    let (pid_r, pid_w) = pipe()?;
    let (gate_r, gate_w) = pipe()?;
    let (pid_r_raw, gate_w_raw) = (pid_r.as_raw_fd(), gate_w.as_raw_fd());

    let mut cmd = command(argv)?;
    // SAFETY: the closure only calls async-signal-safe functions and does not allocate.
    unsafe {
        cmd.pre_exec(move || {
            // The child holds copies of the parent's ends; keeping `gate_w` would hide a parent
            // that gave up, because the gate would never reach end of file.
            let _ = unistd::close(pid_r_raw);
            let _ = unistd::close(gate_w_raw);
            unistd::write(&pid_w, &unistd::getpid().as_raw().to_ne_bytes())
                .map_err(io::Error::from)?;
            let mut byte = [0u8; 1];
            loop {
                match unistd::read(&gate_r, &mut byte) {
                    Ok(1) => return Ok(()),
                    Ok(_) => return Err(io::ErrorKind::UnexpectedEof.into()),
                    Err(Errno::EINTR) => {}
                    Err(err) => return Err(err.into()),
                }
            }
        });
    }

    let spawner = thread::spawn(move || cmd.spawn());
    let mut buf = [0u8; 4];
    File::from(pid_r)
        .read_exact(&mut buf)
        .map_err(ctx("reading child pid"))?;
    let pid = u32::from_ne_bytes(buf);

    let adopted = adopt(unit, pid);
    if adopted.is_ok() {
        File::from(gate_w)
            .write_all(&[1])
            .map_err(ctx("releasing child"))?;
    } else {
        // Closing the gate makes the pending child fail instead of running outside the scope.
        drop(gate_w);
    }
    let child = spawner
        .join()
        .map_err(|_| Error::new("spawn thread panicked"))?
        .map_err(ctx("spawning process"));
    adopted?;
    Ok(Started {
        pid,
        waiter: Waiter::Child(child?),
    })
}

fn adopt(unit: &str, pid: u32) -> Result<()> {
    dbus::start_scope(unit, pid)?;
    for _ in 0..50 {
        let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
        if cgroup.contains(unit) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(Error::new(format!("pid {pid} did not join {unit}")))
}
