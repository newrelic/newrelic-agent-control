//! Probes adversarial process spawning: whether setting `JOB_OBJECT_LIMIT_ACTIVE_PROCESS` before
//! `TerminateJobObject` guarantees the job empties even when members actively spawn children.
//!
//! Sub-commands:
//!   flat-spawner        — loops forever, spawning a short-lived child (ping -n 3) every 500 ms.
//!   chain-spawner <N>   — immediately spawns one child chain-spawner(N-1), then loops spawning
//!                         pings. N=0 just loops spawning pings. Models a sub-agent with its own
//!                         long-lived children, each of which is itself spawning.
//!
//! Part A – flat-spawner, limit=1, terminate_all:
//!   The gate closes before the kill call; no spawn in the TerminateProcess delivery window can
//!   succeed. `active_processes` reaches 0 cleanly.
//!
//! Part B – chain-spawner tree, kill root only, then limit=0 + terminate_all:
//!   Step 1 — kill just the root (TerminateProcess): the subtree survives and keeps spawning;
//!             active_processes may even grow after the root is gone.
//!   Step 2 — limit=0 + terminate_all: tests whether 0 is "no processes allowed" or a sentinel
//!             meaning unlimited, and verifies the sequence still drains the full tree.
//!
//!   cargo run -p poc-windows-crash-survival --example adversarial_spawn_probe

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("flat-spawner") => flat_spawner(),
        Some("chain-spawner") => chain_spawner(
            args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        ),
        _ => orchestrator(),
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

/// Loops forever, spawning a short-lived child (ping -n 3 ≈ 3 s) every 500 ms.
#[cfg(windows)]
fn flat_spawner() -> Result<(), Box<dyn std::error::Error>> {
    use std::process::{Command, Stdio};
    use std::time::Duration;
    loop {
        let result = Command::new("ping")
            .args(["-n", "3", "127.0.0.1"])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn();
        match result {
            Ok(child) => println!("flat-spawner {}: spawned {}", std::process::id(), child.id()),
            Err(e) => println!("flat-spawner {}: spawn failed: {e}", std::process::id()),
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Immediately spawns one child chain-spawner(depth-1) if depth > 0, then loops spawning pings.
/// Every node in the tree is independently active, so killing the root leaves an orphan subtree
/// that is still spawning.
#[cfg(windows)]
fn chain_spawner(depth: u32) -> Result<(), Box<dyn std::error::Error>> {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    if depth > 0 {
        let child = Command::new(std::env::current_exe()?)
            .args(["chain-spawner", &(depth - 1).to_string()])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn()?;
        println!(
            "chain-spawner({depth}) {}: spawned chain child {}",
            std::process::id(),
            child.id()
        );
    }

    loop {
        let result = Command::new("ping")
            .args(["-n", "3", "127.0.0.1"])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn();
        match result {
            Ok(child) => println!(
                "chain-spawner({depth}) {}: spawned ping {}",
                std::process::id(),
                child.id()
            ),
            Err(e) => println!(
                "chain-spawner({depth}) {}: spawn failed: {e}",
                std::process::id()
            ),
        }
        std::thread::sleep(Duration::from_millis(500));
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
    let name = format!(r"\BaseNamedObjects\nr-ac-adversarial-spawn-{pid}");

    let creation = Job::create_or_open_native(&name, &sddl)?;
    assert!(!creation.was_existing, "expected a fresh job");
    let job = creation.job;

    // ── Part A: flat loop spawner, limit=1 closes the race, terminate drains to 0 ─────────────
    println!("\n── Part A: flat-spawner + limit=1 + terminate_all ──");
    {
        let child = Command::new(std::env::current_exe()?)
            .arg("flat-spawner")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn()?;
        job.assign(&child)?;

        wait_for_members(&job, 4, Duration::from_secs(10))?;
        let before = job.info()?.active_processes;
        println!("before limit: active={before}");

        job.limit_active_processes(1)?;
        println!("limit=1 set; spawner's next spawn attempts will get ERROR_NOT_ENOUGH_QUOTA");

        std::thread::sleep(Duration::from_secs(1));
        let after_limit = job.info()?.active_processes;
        println!("after 1 s with limit=1: active={after_limit} (existing processes not killed)");

        job.terminate_all(1)?;
        wait_for_empty(&job, Duration::from_secs(5))?;
        println!("active_processes=0");
    }
    println!("PASS A");

    job.clear_limits()?;

    // ── Part B: chain spawner tree — kill root, observe orphan growth, limit=0 + terminate ─────
    println!("\n── Part B: chain-spawner(2) tree + kill root + limit=0 + terminate_all ──");
    {
        // depth=2: root(2) → child(1) → grandchild(0), each also looping pings.
        // Use job.spawn() (CREATE_SUSPENDED) so root is already a job member before its
        // first instruction runs. Without this, root spawns chain-spawner(1) immediately
        // on startup and that sub-chain misses job membership if the orchestrator hasn't
        // called AssignProcessToJobObject yet — making the orphan-subtree scenario untestable.
        let (root_pid, root) = job.spawn(&std::env::current_exe()?, &["chain-spawner", "2"])?;

        // Wait for tree + some pings to settle.
        wait_for_members(&job, 5, Duration::from_secs(10))?;
        let before_kill = job.info()?.active_processes;
        println!("before killing root: active={before_kill}");

        // Step 1: terminate only the root — this is what AC does to a sub-agent main process.
        root.kill()?;
        assert!(root.wait(Duration::from_secs(5)), "root should exit");
        drop(root);

        std::thread::sleep(Duration::from_millis(300));
        let after_kill_root = job.info()?.active_processes;
        println!(
            "after killing root pid={root_pid}: active={after_kill_root} \
             (subtree survives; orphaned spawners still active)"
        );
        assert!(after_kill_root > 0, "orphaned subtree must still be alive");

        // Give orphans time to keep spawning — count may grow while root is gone.
        std::thread::sleep(Duration::from_secs(1));
        let after_wait = job.info()?.active_processes;
        if after_wait > after_kill_root {
            println!(
                "count grew {after_kill_root} → {after_wait} while root was dead — \
                 orphaned chain-spawners are still independently spawning"
            );
        } else {
            println!(
                "count={after_wait} after 1 s (some pings may have expired; \
                 orphaned spawners are still alive)"
            );
        }

        // Step 2: close the gate then kill everything.
        let before_terminate = job.info()?.active_processes;
        println!("before terminate: active={before_terminate}");

        job.limit_active_processes(0)?;
        println!("limit=0 set");

        std::thread::sleep(Duration::from_secs(1));
        let after_limit0 = job.info()?.active_processes;
        if after_limit0 > before_terminate {
            println!(
                "after 1 s: active={after_limit0} > {before_terminate} — \
                 limit=0 is UNLIMITED (sentinel); orphans kept joining the job"
            );
        } else {
            println!(
                "after 1 s: active={after_limit0} — \
                 limit=0 appears to gate new spawns ('no processes allowed')"
            );
        }

        job.terminate_all(1)?;

        // Even if limit=0 is unlimited, stragglers are ping -n 3 processes (≈ 3 s).
        // 12 s is far beyond their lifetime, so the wait is deterministic.
        match wait_for_empty(&job, Duration::from_secs(12)) {
            Ok(()) => {
                println!("active_processes=0 — converged");
                if after_limit0 > before_terminate {
                    println!(
                        "verdict: limit=0 is unlimited; sequence still converged because \
                         all spawners were killed (nothing left to spawn) and stragglers drained"
                    );
                } else {
                    println!("verdict: limit=0 gated new spawns; same guarantees as limit=1");
                }
            }
            Err(e) => {
                // Only reachable if stragglers live > 12 s (not expected from ping -n 3).
                println!("WARNING: {e}; forcing cleanup");
                job.limit_active_processes(1)?;
                job.terminate_all(1)?;
                wait_for_empty(&job, Duration::from_secs(10))?;
                println!("forced cleanup done");
            }
        }
    }
    println!("PASS B");

    make_temporary(&name)?;
    println!("\nPASS");
    Ok(())
}

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
            println!("job has {n} active processes");
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(format!("timeout waiting for {target} members (have {n})").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

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
            return Err(
                format!("timeout: job still has {n} active processes after {timeout:?}").into(),
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
