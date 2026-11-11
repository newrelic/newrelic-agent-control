//! Exercises the "open or create" path for a deterministic job name across AC restarts.
//!
//! In production AC the job name is derived from the sub-agent ID (not a pid or random suffix),
//! so on restart AC does not enumerate jobs; it calls `NtCreateJobObject` with the known name:
//!   - Fresh start:  `STATUS_SUCCESS`      → newly created, `was_existing = false`
//!   - After crash:  `STATUS_OBJECT_NAME_EXISTS` → existing permanent job, `was_existing = true`
//!
//! Two sub-scenarios run back to back:
//!   A. member still alive  → verify job, adopt, kill, clean up
//!   B. member died while AC was down → detect empty job (would re-launch in production)
//!
//!   cargo run -p poc-windows-crash-survival --example leftover_and_restart_probe

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::{is_elevated, set_privilege};

    println!("elevated: {}", is_elevated().unwrap());
    let held = set_privilege("SeCreatePermanentPrivilege", true).unwrap();
    println!("SeCreatePermanentPrivilege held: {held}");
    if !held {
        eprintln!("FAIL: SeCreatePermanentPrivilege is required, run as SYSTEM or grant the right");
        std::process::exit(1);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("supervisor-alive") => supervisor_alive(&args[1]),
        Some("supervisor-dead") => supervisor_dead(&args[1]),
        _ => orchestrator(),
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

/// Creates the permanent job, assigns a long-running child, and crashes (aborts without running
/// destructors). The child and the permanent job name outlive the supervisor.
#[cfg(windows)]
fn supervisor_alive(nt_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{Job, current_user_sid, restrictive_sddl, set_privilege};
    use std::process::{Command, Stdio};

    set_privilege("SeCreatePermanentPrivilege", true)?;
    let sddl = restrictive_sddl(&current_user_sid()?);
    let creation = Job::create_or_open_native(nt_path, &sddl)?;
    assert!(!creation.was_existing, "job should be new in supervisor-alive");

    let child = Command::new("ping")
        .args(["-n", "300", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    creation.job.assign(&child)?;
    println!(
        "supervisor-alive {}: child {} in {nt_path}",
        std::process::id(),
        child.id()
    );

    // Abort simulates a crash: no destructors, OS closes handles, permanent name persists.
    std::process::abort();
}

/// Creates the permanent job, assigns a child, kills the child while still running, then exits
/// normally. The permanent job name persists (OBJ_PERMANENT) with zero active members.
#[cfg(windows)]
fn supervisor_dead(nt_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{Job, current_user_sid, restrictive_sddl, set_privilege};
    use std::process::{Command, Stdio};

    set_privilege("SeCreatePermanentPrivilege", true)?;
    let sddl = restrictive_sddl(&current_user_sid()?);
    let creation = Job::create_or_open_native(nt_path, &sddl)?;
    assert!(!creation.was_existing, "job should be new in supervisor-dead");

    let mut child = Command::new("ping")
        .args(["-n", "300", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    creation.job.assign(&child)?;
    child.kill()?;
    let _ = child.wait();
    println!(
        "supervisor-dead {}: child killed, exiting normally, {nt_path} persists",
        std::process::id()
    );
    // job handle dropped here; permanent name survives
    Ok(())
}

#[cfg(windows)]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::make_temporary;
    use std::process::Command;

    fn run_scenario_a(nt_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        use poc_windows_crash_survival::{
            AdoptedProcess, Job, READ_CONTROL, current_user_sid, restrictive_sddl,
        };
        use std::time::Duration;
        const JOB_OBJECT_QUERY: u32 = 0x0004;

        let user = current_user_sid()?;
        let sddl = restrictive_sddl(&user);
        let creation = Job::create_or_open_native(nt_path, &sddl)?;
        println!(
            "create_or_open: was_existing={} (STATUS_OBJECT_NAME_EXISTS=0x40000000)",
            creation.was_existing,
        );
        assert!(
            creation.was_existing,
            "the permanent job should have survived the crash"
        );

        // Verify before trusting.
        let job = Job::open_native(nt_path, JOB_OBJECT_QUERY | READ_CONTROL)?;
        let sec = job.security()?;
        println!("owner: {}, sddl: {}", sec.owner, sec.sddl);
        let unexpected = sec.unexpected_grants(&["S-1-5-18", &user]);
        assert!(unexpected.is_empty(), "unexpected grants: {unexpected:?}");
        assert!(
            sec.unexpected_owner(&[&user, "S-1-5-32-544"]).is_none(),
            "unexpected owner"
        );

        // Adopt and kill.
        let info = creation.job.info()?;
        println!("active_processes={}, pids={:?}", info.active_processes, info.pids);
        assert_eq!(info.active_processes, 1, "member should still be running");

        let process = AdoptedProcess::open(info.pids[0] as u32)?;
        assert!(
            !process.wait(Duration::from_millis(200)),
            "adopted child should still be running"
        );
        process.kill()?;
        assert!(
            process.wait(Duration::from_secs(5)),
            "child should exit after kill"
        );
        assert_eq!(creation.job.info()?.active_processes, 0, "job should be empty");
        Ok(())
    }

    fn run_scenario_b(nt_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        use poc_windows_crash_survival::{Job, current_user_sid, restrictive_sddl};

        let user = current_user_sid()?;
        let sddl = restrictive_sddl(&user);
        let creation = Job::create_or_open_native(nt_path, &sddl)?;
        println!("create_or_open: was_existing={}", creation.was_existing);
        assert!(
            creation.was_existing,
            "the permanent job should persist after the supervisor exited"
        );

        let info = creation.job.info()?;
        println!("active_processes={}, pids={:?}", info.active_processes, info.pids);
        assert_eq!(
            info.active_processes, 0,
            "member should have died while AC was down"
        );
        println!("empty job detected: sub-agent is not running, would re-launch");
        Ok(())
    }

    let pid = std::process::id();

    // ── Scenario A: member still alive ───────────────────────────────────────────────────────
    println!("\n── Scenario A: member alive after crash ──");
    let name_a = format!(r"\BaseNamedObjects\nr-ac-restart-probe-a-{pid}");

    let status = Command::new(std::env::current_exe()?)
        .args(["supervisor-alive", &name_a])
        .status()?;
    assert!(!status.success(), "supervisor-alive should have crashed (abort)");

    let cleanup_a = run_scenario_a(&name_a);
    let _ = make_temporary(&name_a);
    cleanup_a?;
    println!("PASS A");

    // ── Scenario B: member died while AC was down ─────────────────────────────────────────────
    println!("\n── Scenario B: member died while AC was down ──");
    let name_b = format!(r"\BaseNamedObjects\nr-ac-restart-probe-b-{pid}");

    let status = Command::new(std::env::current_exe()?)
        .args(["supervisor-dead", &name_b])
        .status()?;
    assert!(status.success(), "supervisor-dead should exit cleanly");

    let cleanup_b = run_scenario_b(&name_b);
    let _ = make_temporary(&name_b);
    cleanup_b?;
    println!("PASS B");

    Ok(())
}
