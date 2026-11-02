//! End-to-end crash survival check using the environment variable tag, no bookkeeping.
//!
//! Run it as a regular user on Linux:
//!
//!   cargo run -p poc-linux-crash-survival --example crash_survival_env_tag
//!
//! The example re-executes itself as a "supervisor" that spawns a simulated sub-agent (a master
//! that forks one worker with the same cmdline and environment) tagged with `NR_AC_*` markers, then
//! aborts. The parent plays the restarted AC: it scans `/proc`, picks the group leader, shows the
//! worker is not mistaken for it, then checks two caveats:
//!
//! - a sub-agent that re-execs with a clean environment is NOT found (AC would lose track of it),
//! - when only the workers survive their master they are reported as orphans and killed.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This PoC only runs on Linux");
}

#[cfg(target_os = "linux")]
const AGENT_ID: &str = "poc-agent";
#[cfg(target_os = "linux")]
const EXEC_ID: &str = "poc-exec";

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("supervisor") => supervisor(&args[1]),
        Some("sim-agent") => sim_agent(),
        _ => orchestrator(),
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

/// The command line the simulated sub-agent is spawned with, which discovery must match exactly.
#[cfg(target_os = "linux")]
fn agent_cmdline() -> Vec<std::ffi::OsString> {
    vec![
        std::env::current_exe().expect("current exe").into(),
        "sim-agent".into(),
    ]
}

/// A simulated sub-agent: a master that forks one worker running the very same command line and
/// inheriting the same environment, then both idle. Plays the role of a pre-fork server.
#[cfg(target_os = "linux")]
fn sim_agent() -> Result<(), Box<dyn std::error::Error>> {
    use std::process::{Command, Stdio};

    if std::env::var_os("NR_POC_WORKER").is_none() {
        Command::new(std::env::current_exe()?)
            .arg("sim-agent")
            .env("NR_POC_WORKER", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
    std::thread::sleep(std::time::Duration::from_secs(300));
    Ok(())
}

/// Spawns the tagged sub-agent and crashes without any cleanup.
#[cfg(target_os = "linux")]
fn supervisor(instance: &str) -> Result<(), Box<dyn std::error::Error>> {
    use poc_linux_crash_survival::{spawn_detached, tag::Identity};
    use std::process::Command;

    let cmdline = agent_cmdline();
    let identity = Identity {
        agent_id: AGENT_ID,
        exec_id: EXEC_ID,
        instance,
        cmdline: &cmdline,
    };
    let mut cmd = Command::new(&cmdline[0]);
    cmd.arg("sim-agent");
    // Tag after the executable's own env so it cannot override the markers.
    identity.tag(&mut cmd);
    spawn_detached(&mut cmd)?;

    // Abort skips destructors; the kernel just drops our resources, as in a real crash.
    std::process::abort();
}

/// Acts as the restarted AC. A failed `assert!` panics, which also exits with a non-zero status.
#[cfg(target_os = "linux")]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use poc_linux_crash_survival::tag::{Identity, kill_orphans, scan, select_root};
    use poc_linux_crash_survival::{is_root, read_stat, spawn_detached, wait_until};
    use std::process::Command;
    use std::time::Duration;

    println!("root: {}", is_root());

    // Step 1: run the supervisor and let it crash. The instance marker is this run's pid, so
    // parallel runs never see each other.
    let instance = format!("/poc/instance-{}", std::process::id());
    let status = Command::new(std::env::current_exe()?)
        .arg("supervisor")
        .arg(&instance)
        .status()?;
    assert!(!status.success(), "supervisor should have crashed");

    // Step 2: the restarted AC scans /proc. It knows nothing but its own identity.
    let cmdline = agent_cmdline();
    let identity = Identity {
        agent_id: AGENT_ID,
        exec_id: EXEC_ID,
        instance: &instance,
        cmdline: &cmdline,
    };
    let found = wait_until(Duration::from_secs(5), || {
        let found = scan(&identity);
        (found.len() == 2).then_some(found)
    })
    .ok_or("expected the master and its worker to survive and be found")?;
    println!("candidates: {found:?}");

    let root = select_root(&found)
        .ok_or("no group leader among the candidates")?
        .clone();
    let worker = found
        .iter()
        .find(|c| c.pid != root.pid)
        .ok_or("no worker")?
        .clone();
    assert_eq!(root.pgid, root.pid, "the root must lead its own group");
    assert_eq!(
        worker.pgid, root.pid,
        "the worker shares the leader's group"
    );
    assert_eq!(worker.ppid, root.pid, "the worker is a child of the master");
    println!("root: {} (worker {} was not picked)", root.pid, worker.pid);

    // Another AC instance must not see our processes.
    let other = Identity {
        instance: "/poc/another-instance",
        ..identity
    };
    assert!(
        scan(&other).is_empty(),
        "a different instance must not match"
    );

    // Step 3, caveat: a sub-agent that re-execs with a clean env loses the tag and is not found.
    let mut scrubbed = Command::new("env");
    scrubbed.args(["-i", "sleep", "300"]);
    identity.tag(&mut scrubbed);
    let mut scrubbed = spawn_detached(&mut scrubbed)?;
    let sleep_cmdline = ["sleep".into(), "300".into()];
    let sleep_identity = Identity {
        cmdline: &sleep_cmdline,
        ..identity
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        scan(&sleep_identity).is_empty(),
        "a process that scrubbed its environment must not be found: AC would lose track of it"
    );
    println!(
        "caveat confirmed: `env -i` child {} is invisible to the scan",
        scrubbed.id()
    );
    scrubbed.kill()?;
    scrubbed.wait()?;

    // Step 4: kill only the master. The worker survives without its leader.
    kill(Pid::from_raw(root.pid as i32), Signal::SIGKILL)?;
    let orphans = wait_until(Duration::from_secs(5), || {
        read_stat(root.pid)
            .is_none_or(|s| matches!(s.state, 'Z' | 'X'))
            .then_some(())?;
        let found = scan(&identity);
        (found.len() == 1).then_some(found)
    })
    .ok_or("the worker should outlive its master")?;
    assert!(
        select_root(&orphans).is_none(),
        "orphaned workers must not be adopted"
    );
    println!("orphaned workers: {orphans:?}");

    // They are killed before AC respawns, so they do not run unsupervised next to the new copy.
    kill_orphans(&orphans)?;
    wait_until(Duration::from_secs(5), || {
        scan(&identity).is_empty().then_some(())
    })
    .ok_or("the orphaned worker should be gone")?;

    println!("PASS: found by tag, leader picked over worker, scrubbed env lost, orphans killed");
    Ok(())
}
