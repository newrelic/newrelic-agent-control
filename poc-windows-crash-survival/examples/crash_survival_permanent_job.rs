//! End-to-end crash survival check where a permanent named job is the discovery mechanism, with
//! no `(pid, start_time)` bookkeeping.
//!
//!   cargo run -p poc-windows-crash-survival --example crash_survival_permanent_job
//!
//! Needs `SeCreatePermanentPrivilege` (held by `SYSTEM`, or granted to a user by an
//! administrator). Without it the example reports that and exits with a non-zero status.
//!
//! The example re-executes itself as a "supervisor" that creates a permanent named job with a
//! restrictive DACL, assigns a long-running child to it and aborts (simulating an AC crash).
//! The parent plays the restarted AC: it only knows the job name. It lists the named jobs,
//! finds the orphaned permanent one (no handle held by anybody), reopens it, checks owner and
//! DACL, adopts the members, kills them and makes the job temporary so the name does not leak.

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("supervisor") => supervisor(&args[1]),
        _ => orchestrator(),
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

/// Creates the permanent job, starts the child inside it and crashes without any cleanup.
#[cfg(windows)]
fn supervisor(nt_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{Job, current_user_sid, restrictive_sddl, set_privilege};
    use std::process::{Command, Stdio};

    set_privilege("SeCreatePermanentPrivilege", true)?;
    let job = Job::create_native_with_sddl(nt_path, true, &restrictive_sddl(&current_user_sid()?))?;
    let child = Command::new("ping")
        .args(["-n", "300", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Assign right after spawn, before any other work.
    job.assign(&child)?;
    println!(
        "supervisor {}: child {} in {nt_path}",
        std::process::id(),
        child.id()
    );

    // Abort skips destructors, the OS closes our handles as in a real crash.
    std::process::abort();
}

/// Acts as the restarted AC. A failed `assert!` panics, which also exits with a non-zero status.
#[cfg(windows)]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{
        AdoptedProcess, Job, READ_CONTROL, current_user_sid, is_elevated, list_named_jobs,
        make_temporary, set_privilege,
    };
    use std::process::Command;
    use std::time::Duration;

    const DIRECTORY: &str = r"\BaseNamedObjects";
    const JOB_OBJECT_QUERY: u32 = 0x0004;

    println!("elevated: {}", is_elevated()?);
    let held = set_privilege("SeCreatePermanentPrivilege", true)?;
    println!("SeCreatePermanentPrivilege held: {held}");
    if !held {
        return Err(
            "SeCreatePermanentPrivilege is required, run as SYSTEM or grant the right".into(),
        );
    }

    // The restarted AC derives the name from configuration (the agent id in the real AC). The
    // pid only keeps repeated runs apart.
    let name = format!("nr-ac-crash-survival-{}", std::process::id());
    let nt_path = format!(r"{DIRECTORY}\{name}");

    // Step 1: run the supervisor in a separate process and let it crash. Nothing is bookkept.
    let status = Command::new(std::env::current_exe()?)
        .arg("supervisor")
        .arg(&nt_path)
        .status()?;
    assert!(!status.success(), "supervisor should have crashed");

    // From here on, make sure the permanent name does not leak even if a check fails.
    let outcome = (|| -> Result<(), Box<dyn std::error::Error>> {
        // Step 2: find the orphaned job. A permanent job that nobody holds a handle to and
        // that still has members is what a crash leaves behind.
        let listing = list_named_jobs(DIRECTORY)?;
        let found = listing
            .jobs
            .iter()
            .find(|job| job.name == name)
            .ok_or("the permanent job should still be listed after the crash")?;
        println!("{found:?}");
        assert!(found.permanent, "the job should be permanent");
        assert_eq!(found.other_handles, 0, "nobody should hold a handle");
        assert_eq!(
            found.active_processes, 1,
            "the child should still be a member"
        );

        // Step 3: reopen the job and verify it is ours before trusting it.
        let job = Job::open_native(&nt_path, JOB_OBJECT_QUERY | READ_CONTROL)?;
        let security = job.security()?;
        println!("owner: {:?}, sddl: {}", security.owner, security.sddl);
        let user = current_user_sid()?;
        let unexpected = security.unexpected_grants(&["S-1-5-18", &user]);
        assert!(unexpected.is_empty(), "unexpected grants: {unexpected:?}");
        assert!(
            security
                .unexpected_owner(&[&user, "S-1-5-32-544"])
                .is_none(),
            "unexpected owner"
        );

        // Step 4: adopt the members straight from the job, no start time involved.
        let info = job.info()?;
        println!("members: {:?}", info.pids);
        assert_eq!(info.pids.len(), 1, "exactly one member expected");
        let process = AdoptedProcess::open(info.pids[0] as u32)?;
        assert!(
            !process.wait(Duration::from_millis(300)),
            "the adopted child should still be running"
        );

        // Step 5: kill it and check the job is empty.
        process.kill()?;
        assert!(
            process.wait(Duration::from_secs(5)),
            "the child should exit after being killed"
        );
        assert!(!process.is_alive()?, "the child should no longer be alive");
        assert_eq!(job.info()?.active_processes, 0, "the job should be empty");
        Ok(())
    })();

    // Step 6: remove the leftover name.
    let cleanup = make_temporary(&nt_path);
    println!("cleanup: {cleanup:?}");
    outcome?;
    cleanup?;
    assert!(
        Job::open_native(&nt_path, JOB_OBJECT_QUERY).is_err(),
        "the name should be gone after make_temporary"
    );

    println!("PASS");
    Ok(())
}
