//! Small supervisor to try the `cgroup-tracker` crate by hand.
//!
//! Each invocation is a separate process, so it shows that a process tree tracked in a cgroup
//! outlives the process that started it and can be found and stopped by a later one.
//!
//! ```text
//! cgroup_supervisor <command>
//!
//! Commands:
//!   start-or-readopt
//!            Spawn `sh -c "sleep 3600 & wait"` in its cgroup and exit without waiting, unless
//!            the cgroup already has live members, in which case they are re-adopted as is.
//!   status   Print the pids that are members of the cgroup.
//!   stop     Kill every member of the cgroup and remove it.
//! ```
//!
//! The hierarchy and base are detected from the current process, so every invocation must run
//! in the same cgroup. Run it as root, for example from a single shell session.

#[cfg(not(unix))]
fn main() {}

#[cfg(unix)]
fn main() {
    if let Err(err) = unix::run(std::env::args().skip(1).collect()) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

#[cfg(unix)]
mod unix {
    use cgroup_tracker::{CgroupName, CgroupTracker};
    use std::process::{Command, Stdio};

    /// Script run by `start-or-readopt`: a shell that keeps a background `sleep` child alive.
    const SCRIPT: &str = "sleep 3600 & wait";

    const USAGE: &str = "usage: cgroup_supervisor <start-or-readopt|status|stop>";

    pub fn run(args: Vec<String>) -> Result<(), String> {
        let [command] = args.as_slice() else {
            return Err(USAGE.to_string());
        };

        let tracker = CgroupTracker::detect().map_err(|e| e.to_string())?;
        println!(
            "base: {} ({:?})",
            tracker.base().display(),
            tracker.hierarchy()
        );

        let name = CgroupName::try_from("example-agent-sleep-tree").map_err(|e| e.to_string())?;
        match command.as_str() {
            "start-or-readopt" => start_or_readopt(&tracker, &name),
            "status" => status(&tracker, &name),
            "stop" => stop(&tracker, &name),
            _ => Err(USAGE.to_string()),
        }
    }

    fn start_or_readopt(tracker: &CgroupTracker, name: &CgroupName) -> Result<(), String> {
        let cgroup = tracker.cgroup(name);
        let path = cgroup.path();
        let members = cgroup.members().map_err(|e| e.to_string())?;
        if !members.is_empty() {
            println!("cgroup: {}", path.display());
            println!("re-adopted existing tree, members: {members:?}");
            println!("not spawning a new process");
            return Ok(());
        }

        let mut command = Command::new("sh");
        command
            .args(["-c", SCRIPT])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Creates the cgroup and makes the child join it before `exec`.
        let child = cgroup.spawn(command).map_err(|e| e.to_string())?;
        println!("spawned `sh -c '{SCRIPT}'` with pid {}", child.id());
        println!("cgroup: {}", path.display());
        // The child is neither waited for nor killed: exiting here is what a crash looks like.
        println!("exiting without stopping the tree");
        Ok(())
    }

    fn status(tracker: &CgroupTracker, name: &CgroupName) -> Result<(), String> {
        let cgroup = tracker.cgroup(name);
        let path = cgroup.path();
        if !path.exists() {
            println!("cgroup {} does not exist", path.display());
            return Ok(());
        }
        let pids = cgroup.members().map_err(|e| e.to_string())?;
        println!("cgroup: {}", path.display());
        println!("members: {}", pids.len());
        for pid in pids {
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            println!("  {pid}: {}", cmdline.trim());
        }
        Ok(())
    }

    fn stop(tracker: &CgroupTracker, name: &CgroupName) -> Result<(), String> {
        let cgroup = tracker.cgroup(name);
        cgroup.kill_and_remove().map_err(|e| e.to_string())?;
        println!("killed and removed {}", cgroup.path().display());
        Ok(())
    }
}
