//! Probes process-tree termination: does the permanent job capture a sub-agent's own spawned
//! children, and which kill strategy cleans up the full tree?
//!
//! The example re-executes itself in worker modes. The orchestrator runs three scenarios:
//!
//!   Part A – Auto-join: children of a job member automatically inherit job membership.
//!   After the sub-agent spawns its two children the job reports 3 active processes.
//!
//!   Part B – TerminateProcess on the top-level process leaves the two children alive as
//!   orphans. `job.info().active_processes` drops from 3 to 2, not 0.
//!   AC cannot rely on this to clean up a sub-agent's process tree.
//!
//!   Part C – `job.terminate_all()` (`TerminateJobObject`) kills every member at once.
//!   After the call `active_processes` is 0 regardless of how deep the tree is.
//!   This is the correct kill primitive for AC.
//!
//!   Part D – CREATE_BREAKAWAY_FROM_JOB: without JOB_OBJECT_LIMIT_BREAKAWAY_OK on the job,
//!   a member process cannot spawn breakaway children (ERROR_ACCESS_DENIED). With the limit
//!   set the child spawns successfully and is NOT a job member; it survives `terminate_all`.
//!   The test is proxied through the sub-agent because only a job MEMBER can test breakaway.
//!
//!   cargo run -p poc-windows-crash-survival --example sub_process_tree_probe

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::set_privilege;

    set_privilege("SeCreatePermanentPrivilege", true).unwrap();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("sub-agent") => sub_agent(),
        Some("sub-agent-breakaway") => sub_agent_breakaway(),
        _ => orchestrator(),
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

/// Spawns two long-running children and blocks until killed.
#[cfg(windows)]
fn sub_agent() -> Result<(), Box<dyn std::error::Error>> {
    use std::process::{Command, Stdio};

    let c1 = Command::new("ping")
        .args(["-n", "300", "127.0.0.1"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()?;
    let c2 = Command::new("ping")
        .args(["-n", "300", "127.0.0.1"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()?;
    println!("sub-agent {} spawned children {} {}", std::process::id(), c1.id(), c2.id());
    std::thread::sleep(std::time::Duration::from_secs(120));
    Ok(())
}

/// Tries to spawn a child with CREATE_BREAKAWAY_FROM_JOB.
/// Exits 0 on success, 5 on ERROR_ACCESS_DENIED, 1 on unexpected error.
///
/// Must be assigned to a job by the orchestrator before it runs. It sleeps briefly at startup
/// to give the orchestrator time to call `job.assign()` before it attempts the spawn.
#[cfg(windows)]
fn sub_agent_breakaway() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    // Give the orchestrator time to assign this process to the job before we test breakaway.
    std::thread::sleep(std::time::Duration::from_millis(200));

    let result = Command::new("ping")
        .args(["-n", "1", "127.0.0.1"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
        .spawn();

    match result {
        Ok(mut child) => {
            let pid = child.id();
            let _ = child.wait();
            println!("sub-agent-breakaway {}: breakaway child {pid} spawned and finished", std::process::id());
            std::process::exit(0);
        }
        Err(e) if e.raw_os_error() == Some(5) => {
            println!("sub-agent-breakaway {}: ERROR_ACCESS_DENIED (expected without BREAKAWAY_OK)", std::process::id());
            std::process::exit(5);
        }
        Err(e) => {
            eprintln!("sub-agent-breakaway: unexpected error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{
        Job, current_user_sid, make_temporary, restrictive_sddl, set_privilege,
    };
    use std::process::{Command, Stdio};
    use std::time::Duration;

    set_privilege("SeCreatePermanentPrivilege", true)?;
    let user = current_user_sid()?;
    let sddl = restrictive_sddl(&user);
    let pid = std::process::id();
    let name = format!(r"\BaseNamedObjects\nr-ac-subtree-probe-{pid}");

    let creation = Job::create_or_open_native(&name, &sddl)?;
    assert!(!creation.was_existing);
    let job = creation.job;

    // ── Part A: children auto-join the job ────────────────────────────────────────────────────
    println!("\n── Part A: children of a job member auto-join the job ──");
    {
        let (sub_pid, _proc) = spawn_sub_agent(&job, "sub-agent")?;
        wait_for_members(&job, 3, Duration::from_secs(10))?;
        let info = job.info()?;
        assert!(
            info.active_processes >= 3,
            "expected sub-agent + 2 children in job, got {}",
            info.active_processes
        );
        assert!(info.pids.contains(&(sub_pid as usize)));
        println!(
            "active_processes={} (sub-agent + 2 children), all in job",
            info.active_processes
        );
        job.terminate_all(1)?;
        wait_for_empty(&job, Duration::from_secs(5))?;
    }
    println!("PASS A");

    // ── Part B: TerminateProcess on top-level leaves children alive ───────────────────────────
    println!("\n── Part B: TerminateProcess on top-level leaves children alive ──");
    {
        let (_, process) = spawn_sub_agent(&job, "sub-agent")?;
        wait_for_members(&job, 3, Duration::from_secs(10))?;
        let before = job.info()?.active_processes;

        process.kill()?;
        assert!(process.wait(Duration::from_secs(5)));

        std::thread::sleep(Duration::from_millis(200));
        let after = job.info()?.active_processes;
        println!("before={before} active, after TerminateProcess on top-level: {after} (children alive)");
        assert!(after >= 2, "expected at least 2 orphaned children, got {after}");
        assert!(after < before);

        job.terminate_all(1)?;
        wait_for_empty(&job, Duration::from_secs(5))?;
    }
    println!("PASS B");

    // ── Part C: terminate_all kills the entire tree ───────────────────────────────────────────
    println!("\n── Part C: terminate_all kills every member including children ──");
    {
        let (_sub_pid, _proc) = spawn_sub_agent(&job, "sub-agent")?;
        wait_for_members(&job, 3, Duration::from_secs(10))?;
        let before = job.info()?.active_processes;
        job.terminate_all(1)?;
        wait_for_empty(&job, Duration::from_secs(5))?;
        println!("before={before}, after terminate_all: 0");
    }
    println!("PASS C");

    // ── Part D: CREATE_BREAKAWAY_FROM_JOB ────────────────────────────────────────────────────
    println!("\n── Part D: CREATE_BREAKAWAY_FROM_JOB requires JOB_OBJECT_LIMIT_BREAKAWAY_OK ──");
    {
        // Without BREAKAWAY_OK: sub-agent-breakaway must fail with ERROR_ACCESS_DENIED (exit 5).
        println!("  without BREAKAWAY_OK:");
        let mut child = Command::new(std::env::current_exe()?)
            .arg("sub-agent-breakaway")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn()?;
        job.assign(&child)?;
        let status = child.wait()?;
        println!("    sub-agent-breakaway exit code: {:?}", status.code());
        assert_eq!(
            status.code(),
            Some(5),
            "expected exit 5 (ERROR_ACCESS_DENIED) without BREAKAWAY_OK"
        );
        wait_for_empty(&job, Duration::from_secs(5))?;

        // With BREAKAWAY_OK: sub-agent-breakaway must succeed (exit 0).
        println!("  with BREAKAWAY_OK:");
        job.allow_breakaway(true)?;
        let mut child2 = Command::new(std::env::current_exe()?)
            .arg("sub-agent-breakaway")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn()?;
        job.assign(&child2)?;
        let status2 = child2.wait()?;
        println!("    sub-agent-breakaway exit code: {:?}", status2.code());
        assert_eq!(
            status2.code(),
            Some(0),
            "expected exit 0 (breakaway succeeded) with BREAKAWAY_OK"
        );
        wait_for_empty(&job, Duration::from_secs(5))?;
    }
    println!("PASS D");

    make_temporary(&name)?;
    println!("\nPASS");
    Ok(())
}

/// Spawns this binary in `mode` (e.g. "sub-agent") and places it atomically in `job`
/// using `PROC_THREAD_ATTRIBUTE_JOB_LIST` — no spawn→assign race.
#[cfg(windows)]
fn spawn_sub_agent(
    job: &poc_windows_crash_survival::Job,
    mode: &str,
) -> Result<(u32, poc_windows_crash_survival::AdoptedProcess), Box<dyn std::error::Error>> {
    Ok(job.spawn(&std::env::current_exe()?, &[mode])?)
}

/// Polls until `active_processes >= target` or timeout.
#[cfg(windows)]
fn wait_for_members(
    job: &poc_windows_crash_survival::Job,
    target: u32,
    timeout: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let n = job.info()?.active_processes;
        if n >= target {
            println!("job has {n} active processes: {:?}", job.info()?.pids);
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(format!("timeout waiting for {target} members (have {n})").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Polls until `active_processes == 0` or timeout.
#[cfg(windows)]
fn wait_for_empty(
    job: &poc_windows_crash_survival::Job,
    timeout: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let n = job.info()?.active_processes;
        if n == 0 {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(format!("timeout: job still has {n} active processes").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
