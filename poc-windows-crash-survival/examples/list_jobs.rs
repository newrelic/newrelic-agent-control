//! Lists and filters named job objects, including permanent ones that no process holds a handle
//! to. This is how a restarted AC could find leftover jobs of its sub-agents.
//!
//!   cargo run -p poc-windows-crash-survival --example list_jobs -- [options]
//!
//! Options:
//!   --prefix <p>   only show jobs whose name starts with `p` (default `nr-ac-list-demo-`)
//!   --permanent    only show permanent jobs
//!   --no-demo      do not create the demo jobs, just list what exists
//!
//! Unless `--no-demo` is given, three demo jobs are created in `\BaseNamedObjects`:
//!   - `<prefix>permanent-orphan`: permanent, one running member, every handle closed
//!   - `<prefix>temporary-held`: temporary, kept alive by this process
//!   - `other-list-demo-decoy`: temporary, does not match the default prefix
//!
//! The permanent job needs `SeCreatePermanentPrivilege`; without it that demo job is skipped.

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::{
        Job, NamedJob, is_elevated, list_named_jobs, make_temporary, set_privilege,
    };
    use std::process::{Command, Stdio};
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::GetCurrentProcessId;

    let mut prefix = "nr-ac-list-demo-".to_string();
    let mut only_permanent = false;
    let mut demo = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--prefix" => prefix = args.next().expect("--prefix needs a value"),
            "--permanent" => only_permanent = true,
            "--no-demo" => demo = false,
            other => {
                eprintln!("unknown option {other}");
                std::process::exit(2);
            }
        }
    }

    let mut session = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.unwrap();
    println!(
        "elevated: {:?}, session: {session}",
        is_elevated().map_err(|e| e.to_string())
    );

    // Demo jobs, kept in scope until the end so the temporary ones stay alive.
    let mut held = Vec::new();
    let mut child = None;
    let mut permanent_path = None;
    if demo {
        let held_privilege = set_privilege("SeCreatePermanentPrivilege", true);
        println!("SeCreatePermanentPrivilege held: {held_privilege:?}");

        if matches!(held_privilege, Ok(true)) {
            let path = format!(r"\BaseNamedObjects\{prefix}permanent-orphan");
            match Job::create_native(&path, true) {
                Ok(job) => {
                    let process = Command::new("ping")
                        .args(["-n", "60", "127.0.0.1"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .unwrap();
                    job.assign(&process).unwrap();
                    child = Some(process);
                    permanent_path = Some(path);
                    // Dropping `job` closes the last handle: the name survives only because it is permanent.
                }
                Err(e) => println!("permanent demo job skipped: {e}"),
            }
        }
        for name in [
            format!("{prefix}temporary-held"),
            "other-list-demo-decoy".into(),
        ] {
            match Job::create_native(&format!(r"\BaseNamedObjects\{name}"), false) {
                Ok(job) => held.push(job),
                Err(e) => println!("demo job {name} skipped: {e}"),
            }
        }
    }

    let mut directories = vec![r"\BaseNamedObjects".to_string()];
    if session != 0 {
        directories.push(format!(r"\Sessions\{session}\BaseNamedObjects"));
    }

    let show = |job: &NamedJob| {
        println!(
            "  {} permanent={} other_handles={} active_processes={} pids={:?}",
            job.path, job.permanent, job.other_handles, job.active_processes, job.pids
        );
    };

    for directory in &directories {
        println!("--- {directory}");
        let listing = match list_named_jobs(directory) {
            Ok(listing) => listing,
            Err(e) => {
                println!("  listing failed: {e}");
                continue;
            }
        };
        println!(
            "[all] jobs: {}, unreadable jobs: {}, entries of other types: {}",
            listing.jobs.len(),
            listing.unreadable,
            listing.other_types
        );
        for job in &listing.jobs {
            show(job);
        }

        println!("[filter: name starts with {prefix:?}, permanent only: {only_permanent}]");
        let filtered = listing
            .jobs
            .iter()
            .filter(|job| job.name.starts_with(&prefix) && (!only_permanent || job.permanent))
            .collect::<Vec<_>>();
        for job in &filtered {
            show(job);
        }
        println!("  matches: {}", filtered.len());

        let orphans = filtered
            .iter()
            .filter(|job| job.permanent && job.other_handles == 0)
            .count();
        println!("  permanent jobs nobody holds a handle to: {orphans}");
    }

    // The permanent job must be turned back into a temporary one, or its name stays until reboot.
    if let Some(path) = permanent_path {
        println!(
            "cleanup: make_temporary({path}): {:?}",
            make_temporary(&path).map_err(|e| e.to_string())
        );
    }
    if let Some(mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
    }
    drop(held);
}
