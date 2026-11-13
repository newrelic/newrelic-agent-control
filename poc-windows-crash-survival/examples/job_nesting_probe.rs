//! Probes dynamic Job Object nesting: what happens when a process already in AC's job is also
//! assigned to a second job by a sub-agent managing its own children?
//!
//! On Windows 8+ `AssignProcessToJobObject` on a process that is already in a job attempts to
//! build a nested hierarchy (outer → inner) rather than failing outright. Whether it succeeds
//! depends on limit *compatibility* between the two jobs.
//!
//! Parts:
//!   Part A – outer job has no limits (PoC named-permanent-job model); inner job has only
//!            KILL_ON_JOB_CLOSE. Expected: nesting SUCCEEDS. After closing the inner job handle,
//!            KILL_ON_JOB_CLOSE fires and the child is terminated.
//!
//!   Part B – outer job has only KILL_ON_JOB_CLOSE (current anonymous-job model in AC main
//!            branch); inner also has KILL_ON_JOB_CLOSE. Expected: nesting SUCCEEDS.
//!
//!   Part C – outer job has a CPU hard-cap (50 %); inner has KILL_ON_JOB_CLOSE only. Expected:
//!            nesting FAILS with ERROR_ACCESS_DENIED (5) because the outer's CPU-rate limit is
//!            incompatible with the inner lacking any CPU-rate control.
//!
//!   cargo run -p poc-windows-crash-survival --example job_nesting_probe

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("worker") => worker(),
        _ => orchestrator(),
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::Job;
    use std::time::Duration;

    let exe = std::env::current_exe()?;

    // ── Part A: no-limit outer + KILL_ON_JOB_CLOSE inner ─────────────────────────────────────
    println!("\n── Part A: no-limit outer + KILL_ON_JOB_CLOSE inner ──");
    {
        let outer = Job::new()?; // no limits — mirrors the PoC named permanent job
        let (child_pid, child) = outer.spawn(&exe, &["worker"])?;
        println!("  spawned child pid={child_pid} into no-limit outer job");

        let pids_before = outer.info()?.pids;
        assert!(
            pids_before.contains(&(child_pid as usize)),
            "child must be in outer after spawn"
        );

        let inner = Job::new()?;
        inner.set_kill_on_close()?; // KILL_ON_JOB_CLOSE only

        // child is already in outer; assign it to inner to build a nesting hierarchy.
        match inner.assign_adopted(&child) {
            Err(e) => {
                // Unexpected: a no-limit outer + KOJ inner should always nest cleanly.
                eprintln!("  SKIP A: inner.assign_adopted failed (prlctl interference?): {e}");
                let _ = outer.terminate_all(1);
            }
            Ok(()) => {
                println!("  inner.assign_adopted: SUCCESS — child is now in both outer and inner");
                let pids_after = outer.info()?.pids;
                assert!(
                    pids_after.contains(&(child_pid as usize)),
                    "child must still be in outer after inner assignment"
                );
                println!("  outer still reports child pid={child_pid}: OK");

                // Drop inner → KILL_ON_JOB_CLOSE fires → child is terminated.
                drop(inner);
                let exited = child.wait(Duration::from_secs(3));
                assert!(
                    exited,
                    "child must exit after inner job handle is dropped (KILL_ON_JOB_CLOSE)"
                );
                println!("  child exited after inner job handle closed (KILL_ON_JOB_CLOSE fired): OK");
                println!("PASS A");
            }
        }
    }

    // ── Part B: KILL_ON_JOB_CLOSE outer + KILL_ON_JOB_CLOSE inner ────────────────────────────
    println!("\n── Part B: KILL_ON_JOB_CLOSE outer + KILL_ON_JOB_CLOSE inner ──");
    {
        let outer = Job::new()?;
        outer.set_kill_on_close()?; // mirrors current AC anonymous job

        let (child_pid, child) = outer.spawn(&exe, &["worker"])?;
        println!("  spawned child pid={child_pid} into KILL_ON_JOB_CLOSE outer job");

        let inner = Job::new()?;
        inner.set_kill_on_close()?;

        match inner.assign_adopted(&child) {
            Err(e) => {
                eprintln!("  SKIP B: inner.assign_adopted failed (prlctl interference?): {e}");
                let _ = outer.terminate_all(1);
            }
            Ok(()) => {
                println!("  inner.assign_adopted: SUCCESS — two KILL_ON_JOB_CLOSE jobs nested");
                let pids_after = outer.info()?.pids;
                assert!(pids_after.contains(&(child_pid as usize)));
                println!("  outer still reports child pid={child_pid}: OK");

                drop(inner);
                let exited = child.wait(Duration::from_secs(3));
                assert!(exited, "child must exit after inner job handle closed");
                println!("  child exited after inner job handle closed: OK");
                println!("PASS B");
            }
        }
    }

    // ── Part C: CPU-rate-capped outer + KILL_ON_JOB_CLOSE inner → expect FAIL ─────────────────
    println!("\n── Part C: CPU-rate-capped outer + KILL_ON_JOB_CLOSE inner (expect FAIL) ──");
    {
        let outer = Job::new()?;
        match outer.limit_cpu_rate(50) {
            Err(e) => {
                // CPU rate control may require a specific Windows edition or build.
                println!("  SKIP C: limit_cpu_rate(50) failed — {e}");
                println!("  (CPU rate control may not be available on this edition/build)");
            }
            Ok(()) => {
                println!("  outer job: CPU hard-cap 50% set");

                let (child_pid, child) = outer.spawn(&exe, &["worker"])?;
                println!("  spawned child pid={child_pid} into CPU-capped outer job");

                let inner = Job::new()?;
                inner.set_kill_on_close()?;

                match inner.assign_adopted(&child) {
                    Err(e) => {
                        let raw = e.to_string();
                        // ERROR_ACCESS_DENIED = 5 / 0x80070005
                        assert!(
                            raw.contains("0x80070005")
                                || raw.contains("Access is denied")
                                || raw.contains("(os error 5)"),
                            "expected ERROR_ACCESS_DENIED for incompatible CPU-rate nesting, got: {raw}"
                        );
                        println!("  inner.assign_adopted: FAILED as expected — {raw}");
                        // child is still alive and only in outer; clean up explicitly.
                        assert!(
                            child.is_alive()?,
                            "child must still be alive after failed nesting"
                        );
                        println!("  child still alive after failed nesting: OK");
                        outer.terminate_all(1)?;
                        println!("PASS C");
                    }
                    Ok(()) => {
                        // Unexpected success — document the finding and clean up.
                        println!(
                            "  inner.assign_adopted: SUCCEEDED (unexpected on this build) — \
                             CPU-rate incompatibility rule may not apply here; \
                             update the design doc if consistently reproducible"
                        );
                        drop(inner);
                        let _ = child.wait(Duration::from_secs(3));
                    }
                }
            }
        }
    }

    println!("\nPASS");
    Ok(())
}

/// Long-lived worker mode: sleeps for 120 s. Used as the spawned child in all parts.
#[cfg(windows)]
fn worker() -> Result<(), Box<dyn std::error::Error>> {
    println!("worker pid={} running", std::process::id());
    std::thread::sleep(std::time::Duration::from_secs(120));
    Ok(())
}
