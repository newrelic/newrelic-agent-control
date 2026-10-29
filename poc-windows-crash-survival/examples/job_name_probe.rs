//! Probes what happens to a named job, and what can still be observed about it, once its last
//! handle is closed.
//!
//!   cargo run -p poc-windows-crash-survival --example job_name_probe

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::Job;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    use std::thread::sleep;
    use std::time::Duration;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::IsProcessInJob;
    use windows::core::BOOL;

    let show = |label: &str, job: Result<Job, _>| match job {
        Ok(j) => println!("{label}: {:?}", j.info().map_err(|e| e.to_string())),
        Err(e) => println!("{label}: open failed: {e}"),
    };

    let name = format!("Local\\nr-ac-probe-{}", std::process::id());
    println!("--- name: {name}");

    let mut child = Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let job = match Job::create(&name) {
        Ok(j) => j,
        Err(e) => {
            println!("create failed: {e}");
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
    };
    println!(
        "assign: {:?}",
        job.assign(&child).map_err(|e| e.to_string())
    );
    sleep(Duration::from_millis(200));

    println!("[handle held]");
    println!(
        "  via owned handle: {:?}",
        job.info().map_err(|e| e.to_string())
    );
    show("  via open by name", Job::open(&name));

    drop(job);

    println!("[last handle closed]");
    let mut in_any = BOOL(0);
    let r = unsafe { IsProcessInJob(HANDLE(child.as_raw_handle()), None, &mut in_any) };
    println!("  child in some job: {:?} ({r:?})", in_any.as_bool());
    show("  via open by name", Job::open(&name));

    let _ = child.kill();
    let _ = child.wait();
}
