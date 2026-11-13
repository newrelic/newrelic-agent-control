//! One-shot cleanup: makes temporary every permanent `nr-ac-*` orphan in `\BaseNamedObjects`.
//! An orphan is a permanent job with no active processes and no open handles — left behind
//! when AC (or a probe) exited before calling `NtMakeTemporaryObject`.
//!
//! Accepts an optional `--prefix <p>` argument (default: `nr-ac-`).
//!
//!   cargo run -p poc-windows-crash-survival --example cleanup_orphaned_jobs
//!   cargo run -p poc-windows-crash-survival --example cleanup_orphaned_jobs -- --prefix nr-ac-subtree-probe-

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::{list_named_jobs, make_temporary, set_privilege};

    let mut prefix = "nr-ac-".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--prefix" {
            prefix = args.next().expect("--prefix requires a value");
        }
    }

    set_privilege("SeCreatePermanentPrivilege", true).unwrap();

    let listing = list_named_jobs(r"\BaseNamedObjects").unwrap();
    let orphans: Vec<_> = listing
        .jobs
        .iter()
        .filter(|j| j.name.starts_with(&prefix) && j.permanent && j.active_processes == 0)
        .collect();

    println!("Found {} orphaned {}* job(s)", orphans.len(), prefix);
    for job in &orphans {
        let result = make_temporary(&job.path);
        println!("  make_temporary({}): {:?}", job.path, result.map_err(|e| e.to_string()));
    }
    println!("Done");
}
