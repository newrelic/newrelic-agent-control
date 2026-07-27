//! Cross-platform primitive for killing a spawned process together with any descendants it
//! spawned, not just the direct child.
use std::process::{Child, Command};

#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObject;

/// Error produced while spawning, attaching to, or killing a process group.
#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct ProcessGroupError(String);

/// Handle to the process group of a spawned child, allowing the whole tree (the child and any
/// descendants it spawned) to be killed together.
pub struct ProcessGroup {
    #[cfg(target_family = "unix")]
    pgid: i32,
    #[cfg(target_family = "windows")]
    job_object: JobObject,
}

impl ProcessGroup {
    /// Spawns `command` as the leader of a new process group and attaches to it, returning the
    /// child together with a handle that can kill the whole group. If the group cannot be
    /// established after a successful spawn, the child is killed and reaped before returning.
    pub fn spawn(mut command: Command) -> Result<(Child, Self), ProcessGroupError> {
        Self::prepare(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| ProcessGroupError(format!("failed to spawn process: {e}")))?;
        match Self::attach(&child) {
            Ok(process_group) => Ok((child, process_group)),
            Err(e) => {
                let _ = child.kill();
                Err(e)
            }
        }
    }

    /// Kills every process in the group.
    pub fn kill(self) -> Result<(), ProcessGroupError> {
        #[cfg(target_family = "unix")]
        {
            use nix::sys::signal::{Signal, killpg};
            use nix::unistd::Pid;

            killpg(Pid::from_raw(self.pgid), Signal::SIGKILL)
                .map_err(|e| ProcessGroupError(e.to_string()))?;
            Ok(())
        }
        #[cfg(target_family = "windows")]
        {
            self.job_object
                .kill()
                .map_err(|e| ProcessGroupError(e.to_string()))
        }
    }

    /// Configures `command` so its spawned process becomes the leader of a new process group.
    /// Must be called before `.spawn()`.
    fn prepare(command: &mut Command) {
        #[cfg(target_family = "unix")]
        {
            use std::os::unix::process::CommandExt;
            const USE_PID_AS_PGID: i32 = 0;
            command.process_group(USE_PID_AS_PGID);
        }
        #[cfg(target_family = "windows")]
        {
            let _ = command;
        }
    }

    /// Attaches to the process group of an already-spawned `child`. Must be called right after a
    /// successful `.spawn()` of a command previously passed to [`ProcessGroup::prepare`].
    fn attach(child: &Child) -> Result<Self, ProcessGroupError> {
        #[cfg(target_family = "unix")]
        {
            // `prepare` made `child` the leader of its own process group, so its pid is the pgid.
            Ok(Self {
                pgid: child.id() as i32,
            })
        }
        #[cfg(target_family = "windows")]
        {
            let job_object = JobObject::new().map_err(|e| ProcessGroupError(e.to_string()))?;
            job_object
                .assign_process(child)
                .map_err(|e| ProcessGroupError(e.to_string()))?;
            Ok(Self { job_object })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::retry::retry;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    #[cfg(target_family = "unix")]
    fn test_process_group_kills_process_tree() {
        let temp_dir = TempDir::new().unwrap();
        let grandchild_pid_file = temp_dir.path().join("grandchild.pid");

        // spawns bash as the child; that bash script then forks (echo $$ > ...; sleep 15) & as a background subshell.
        let mut command = Command::new("bash");
        command.arg("-c").arg(format!(
            "(echo $$ > {}; sleep 15) & wait",
            grandchild_pid_file.display()
        ));

        let (mut child, process_group) =
            ProcessGroup::spawn(command).expect("failed to spawn process group");

        let grandchild_pid: i32 = retry(50, Duration::from_millis(100), || {
            fs::read_to_string(&grandchild_pid_file)
                .ok()
                .and_then(|content| content.trim().parse().ok())
                .ok_or("grandchild pid file not written yet")
        })
        .expect("grandchild never wrote its pid file");

        process_group.kill().expect("failed to kill process group");
        let _ = child.wait();

        retry(50, Duration::from_millis(100), || {
            if is_process_running(grandchild_pid) {
                Err("grandchild still running")
            } else {
                Ok::<(), &str>(())
            }
        })
        .expect("grandchild was not killed");
    }

    #[cfg(target_family = "unix")]
    fn is_process_running(pid: i32) -> bool {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        kill(Pid::from_raw(pid), None).is_ok()
    }

    #[test]
    #[cfg(target_family = "windows")]
    fn test_process_group_kills_process_tree() {
        let temp_dir = TempDir::new().unwrap();
        let grandchild_pid_file = temp_dir.path().join("grandchild.pid");

        // spawns powershell as the child; that script starts "cmd /C timeout /T 15" as a grandchild
        // process (Windows adds it to the same Job Object automatically), writes its pid to a file,
        // then waits on it.
        let mut command = Command::new("powershell");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/C','timeout /T 15' -PassThru; \
                 $p.Id | Out-File -FilePath '{}' -Encoding ascii; \
                 Wait-Process -Id $p.Id",
                grandchild_pid_file.display()
            ));

        let (mut child, process_group) =
            ProcessGroup::spawn(command).expect("failed to spawn process group");

        let grandchild_pid: u32 = retry(50, Duration::from_millis(100), || {
            fs::read_to_string(&grandchild_pid_file)
                .ok()
                .and_then(|content| content.trim().parse().ok())
                .ok_or("grandchild pid file not written yet")
        })
        .expect("grandchild never wrote its pid file");

        process_group.kill().expect("failed to kill process group");
        let _ = child.wait();

        retry(50, Duration::from_millis(100), || {
            if is_process_running(grandchild_pid) {
                Err("grandchild still running")
            } else {
                Ok::<(), &str>(())
            }
        })
        .expect("grandchild was not killed");
    }

    #[cfg(target_family = "windows")]
    fn is_process_running(pid: u32) -> bool {
        use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };

            let mut exit_code = 0u32;
            let running = GetExitCodeProcess(handle, &mut exit_code).is_ok()
                && exit_code == STILL_ACTIVE.0 as u32;
            let _ = CloseHandle(handle);
            running
        }
    }
}
