//! End-to-end crash survival check using bookkeeping (`(pid, start_time)`).
//!
//! Run it as a regular user on Linux:
//!
//!   cargo run -p poc-linux-crash-survival --example crash_survival_bookkeeping
//!
//! The example re-executes itself as a "supervisor" that spawns a long-running child, bookkeeps
//! `(pid, start_time)` and aborts (simulating an AC crash). The parent plays the restarted AC: it
//! re-adopts the child from the bookkept values and kills it. Exits with a non-zero status if any
//! check fails.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This PoC only runs on Linux");
}

#[cfg(target_os = "linux")]
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

/// Spawns the child and crashes without any cleanup.
#[cfg(target_os = "linux")]
fn supervisor(bookkeeping_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use poc_linux_crash_survival::{read_stat, spawn_detached};
    use std::process::Command;

    let child = spawn_detached(Command::new("sleep").arg("300"))?;
    let start_time = read_stat(child.id()).ok_or("child vanished")?.start_time;
    // This is the bookkeeping the real AC would persist at spawn time.
    std::fs::write(
        bookkeeping_path,
        format!("{} {start_time} {}", child.id(), std::process::id()),
    )?;

    // Abort skips destructors; the kernel just drops our resources, as in a real crash.
    std::process::abort();
}

/// Acts as the restarted AC. A failed `assert!` panics, which also exits with a non-zero status.
#[cfg(target_os = "linux")]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_linux_crash_survival::{AdoptedProcess, is_root, read_stat};
    use std::process::Command;
    use std::time::Duration;

    // The PoC must demonstrate that no root rights are needed.
    println!("root: {}", is_root());

    // Step 1: run the supervisor in a separate process and let it crash. The bookkeeping goes
    // through a file, as in the real AC.
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
    let pid: u32 = fields.next().ok_or("missing pid")?.parse()?;
    let recorded_start: u64 = fields.next().ok_or("missing start time")?.parse()?;
    let supervisor_pid: u32 = fields.next().ok_or("missing supervisor pid")?.parse()?;

    // Step 3: from here on we only know the bookkept values.
    // A wrong start time must be rejected (pid reuse), the right one adopted.
    assert!(
        AdoptedProcess::adopt(pid, recorded_start + 1).is_none(),
        "a mismatched start time must not be adopted"
    );
    let process = AdoptedProcess::adopt(pid, recorded_start)
        .ok_or("the child should survive the supervisor crash and be adoptable")?;
    assert!(
        !process.wait(Duration::from_millis(300)),
        "the adopted child should still be running"
    );

    // The dead supervisor is NOT left behind as the parent: the kernel reparents
    // the orphan to init (or the nearest subreaper), so the parent pid is no discovery hint.
    assert!(
        read_stat(supervisor_pid).is_none(),
        "the supervisor should be gone"
    );
    let ppid = read_stat(pid).ok_or("child vanished")?.ppid;
    println!("orphan {pid} was reparented: ppid {ppid} (dead supervisor was {supervisor_pid})");
    assert_ne!(
        ppid, supervisor_pid,
        "the orphan must not keep the dead supervisor as parent"
    );

    // Step 4: killing the adopted process must work without root.
    process.kill()?;
    assert!(
        process.wait(Duration::from_secs(5)),
        "the child should exit after being killed"
    );
    assert!(!process.is_alive(), "the child should no longer be alive");

    println!("PASS: orphan survived the crash, was adopted from (pid, start_time) and killed");
    Ok(())
}
