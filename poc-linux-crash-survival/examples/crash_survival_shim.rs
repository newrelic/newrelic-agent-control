//! End-to-end crash survival check using a per-sub-agent shim.
//!
//! Run it as a regular user on Linux:
//!
//!   cargo run -p poc-linux-crash-survival --example crash_survival_shim
//!
//! The example re-executes itself in three roles. The "supervisor" starts two shims, each the real
//! parent of a sub-agent, and aborts (simulating an AC crash). The shims are `shim` invocations of
//! this same binary. The parent plays the restarted AC: it reconnects to the shims by their
//! deterministic socket path, no scanning, and gets what no other option can provide:
//!
//! - the REAL exit status of a sub-agent that died while AC was down,
//! - the sub-agent's output, which went to a file the shim owns, not to a pipe that broke.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This PoC only runs on Linux");
}

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        // shim <socket> <log> <program> [args...]
        Some("shim") => poc_linux_crash_survival::shim::run(
            args[1].as_ref(),
            args[2].as_ref(),
            &args[3],
            &args[4..],
        )
        .map_err(Into::into),
        Some("supervisor") => supervisor(&args[1]),
        _ => orchestrator(),
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
const LONG_RUNNING: (&str, &str) = ("long", "run");
#[cfg(target_os = "linux")]
const SHORT_LIVED: (&str, &str) = ("short", "lived");

/// Starts one shim for a sub-agent and waits until its socket answers.
#[cfg(target_os = "linux")]
fn start_shim(
    dir: &std::path::Path,
    (agent, exec): (&str, &str),
    program: &str,
    args: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    use poc_linux_crash_survival::{shim, spawn_detached, wait_until};
    use std::process::Command;
    use std::time::Duration;

    let socket = shim::socket_path(dir, agent, exec);
    let log = dir.join(format!("{agent}-{exec}.log"));
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("shim")
        .arg(&socket)
        .arg(&log)
        .arg(program)
        .args(args);
    // The shim is detached from AC: its own group, no stdio shared with AC.
    spawn_detached(&mut cmd)?;
    wait_until(Duration::from_secs(5), || shim::query(&socket).ok())
        .ok_or("the shim did not come up")?;
    Ok(())
}

/// Starts the shims and crashes without any cleanup.
#[cfg(target_os = "linux")]
fn supervisor(dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::Path::new(dir);
    start_shim(dir, LONG_RUNNING, "sleep", &["300"])?;
    // Prints, then exits with code 7 one second later: after the supervisor is already dead.
    start_shim(
        dir,
        SHORT_LIVED,
        "sh",
        &["-c", "echo out; sleep 1; echo err >&2; exit 7"],
    )?;

    // Abort skips destructors; the kernel just drops our resources, as in a real crash.
    std::process::abort();
}

/// Acts as the restarted AC. A failed `assert!` panics, which also exits with a non-zero status.
#[cfg(target_os = "linux")]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_linux_crash_survival::shim::{self, Status};
    use poc_linux_crash_survival::{is_root, wait_until};
    use std::process::Command;
    use std::time::Duration;

    println!("root: {}", is_root());

    // Step 1: run the supervisor and let it crash.
    let dir = std::env::temp_dir().join(format!("nr-ac-poc-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let status = Command::new(std::env::current_exe()?)
        .arg("supervisor")
        .arg(&dir)
        .status()?;
    assert!(!status.success(), "supervisor should have crashed");

    // Step 2: reconnect by the deterministic socket path. Nothing is searched for.
    let long = shim::socket_path(&dir, LONG_RUNNING.0, LONG_RUNNING.1);
    let short = shim::socket_path(&dir, SHORT_LIVED.0, SHORT_LIVED.1);

    let Status::Running { pid } = shim::query(&long)? else {
        return Err("the long-running sub-agent should still be running".into());
    };
    println!("long-running sub-agent {pid} is alive, reached through its shim");

    // The short-lived sub-agent exits with code 7 about one second after the crash.
    let exited = wait_until(Duration::from_secs(10), || match shim::query(&short) {
        Ok(status @ (Status::ExitedCode(_) | Status::ExitedSignal(_))) => Some(status),
        _ => None,
    })
    .ok_or("the short-lived sub-agent should have exited")?;
    println!("short-lived sub-agent exited while AC was down: {exited:?}");
    assert_eq!(
        exited,
        Status::ExitedCode(7),
        "the shim must report the real exit code"
    );

    // Its output went to a file the shim owns, so nothing broke when AC died.
    let log =
        std::fs::read_to_string(dir.join(format!("{}-{}.log", SHORT_LIVED.0, SHORT_LIVED.1)))?;
    assert!(
        log.contains("out") && log.contains("err"),
        "output should be kept, got {log:?}"
    );

    // Step 3: kill the long-running one through its shim and read the real exit status.
    shim::kill_agent(&long)?;
    let killed = wait_until(Duration::from_secs(5), || {
        matches!(shim::query(&long), Ok(Status::ExitedSignal(_))).then_some(())
    });
    assert!(
        killed.is_some(),
        "the shim should report the sub-agent killed by a signal"
    );
    assert_eq!(shim::query(&long)?, Status::ExitedSignal(9));

    // Step 4: cleanup. The shims exit and remove their sockets.
    shim::quit(&long)?;
    shim::quit(&short)?;
    wait_until(Duration::from_secs(5), || {
        (!long.exists() && !short.exists()).then_some(())
    })
    .ok_or("the shims should remove their sockets")?;
    let _ = std::fs::remove_dir_all(&dir);

    println!("PASS: reconnected to the shims, got the real exit status and the output");
    Ok(())
}
