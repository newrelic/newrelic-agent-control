//! End-to-end crash survival check, without named jobs.
//!
//! Run it from a NON-elevated shell on Windows:
//!
//!   cargo run -p poc-windows-crash-survival --example crash_survival
//!
//! The example re-executes itself as a "supervisor" that assigns a long-running child to an
//! anonymous job (no kill-on-close) and then aborts (simulating an AC crash). The parent plays
//! the restarted AC: it re-adopts the child from the bookkept `(pid, start_time)` and kills it.
//! It also checks that the orphan keeps the dead supervisor as its parent pid in the process
//! snapshot, which allows narrowing a recovery scan without opening any process.
//! Exits with a non-zero status if any check fails (failed assertion or error).

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

/// Starts the child inside a job and crashes without any cleanup.
#[cfg(windows)]
fn supervisor(bookkeeping_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{AdoptedProcess, Job};
    use std::process::{Command, Stdio};

    let child = Command::new("ping")
        .args(["-n", "300", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Assign right after spawn, before any other work.
    let job = Job::new()?;
    job.assign(&child)?;

    let start_time = AdoptedProcess::open(child.id())?.start_time()?;
    // This is the bookkeeping the real AC would persist at spawn time.
    std::fs::write(
        bookkeeping_path,
        format!("{} {start_time} {}", child.id(), std::process::id()),
    )?;

    // Abort skips destructors, the OS closes our handles as in a real crash.
    std::process::abort();
}

/// Acts as the restarted AC. A failed `assert!` panics, which also exits with a non-zero status.
#[cfg(windows)]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{AdoptedProcess, find_processes_by_parent, is_elevated};
    use std::process::Command;
    use std::time::Duration;

    // The PoC must demonstrate that no admin rights are needed.
    println!("elevated: {}", is_elevated()?);

    // Step 1: run the supervisor in a separate process and let it crash. The bookkeeping goes
    // through a file, as in the real AC. A pipe would be inherited by the child and keep
    // `output()` blocked until the child exits.
    let bookkeeping = std::env::temp_dir().join(format!("nr-ac-poc-{}.txt", std::process::id()));
    let status = Command::new(std::env::current_exe()?)
        .arg("supervisor")
        .arg(&bookkeeping)
        .status()?;
    assert!(!status.success(), "supervisor should have crashed");

    // Step 2: read the bookkeeping the supervisor wrote: "<pid> <start_time> <supervisor_pid>".
    let contents = std::fs::read_to_string(&bookkeeping)?;
    let _ = std::fs::remove_file(&bookkeeping);
    let mut fields = contents.split_whitespace();
    let pid: u32 = fields.next().ok_or("missing pid in bookkeeping")?.parse()?;
    let recorded_start: u64 = fields.next().ok_or("missing start time")?.parse()?;
    let supervisor_pid: u32 = fields.next().ok_or("missing supervisor pid")?.parse()?;

    // Step 3: from here on we play the restarted AC, which only knows the bookkept values.
    // A wrong start time must be rejected (pid reuse), the right one adopted.
    assert!(
        AdoptedProcess::adopt(pid, recorded_start + 1)?.is_none(),
        "a mismatched start time must not be adopted"
    );
    let process = AdoptedProcess::adopt(pid, recorded_start)?
        .ok_or("the child should survive the supervisor crash and be adoptable")?;
    // A timed-out wait means the process has not exited.
    assert!(
        !process.wait(Duration::from_millis(300)),
        "the adopted child should still be running"
    );

    // The dead supervisor must still show up as the child's parent in the snapshot.
    assert!(
        AdoptedProcess::open(supervisor_pid).is_err(),
        "the supervisor should be gone"
    );
    let children = find_processes_by_parent(supervisor_pid)?;
    println!("children of dead supervisor {supervisor_pid}: {children:?} (child: {pid})");
    assert!(
        children.contains(&pid),
        "the orphan should keep the dead supervisor as parent pid"
    );

    // Step 4: killing the adopted process must work without elevation.
    process.kill()?;
    assert!(
        process.wait(Duration::from_secs(5)),
        "the child should exit after being killed"
    );
    assert!(!process.is_alive()?, "the child should no longer be alive");

    println!("PASS");
    Ok(())
}
