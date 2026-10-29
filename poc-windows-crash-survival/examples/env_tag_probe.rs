//! Probes recovering a child by an environment variable tag. The child is found by scanning for
//! the tag, adopted and killed, with no use of its pid or any handle kept from the spawn.
//!
//!   cargo run -p poc-windows-crash-survival --example env_tag_probe -- [--tag <value>]
//!
//! It runs a full scan and then a scan restricted by image name, to compare how many processes
//! are opened.
//!
//! `--tag` is passed to the child only as the `NR_AC_TAG` environment variable, since real
//! sub-agents may not accept arbitrary args.

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::{
        AdoptedProcess, find_processes_by_env_tag, is_elevated, process_env_var,
    };
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    // Executables the sub-agent could be; here the probe child is `ping`.
    const IMAGE_FILTER: &[&str] = &["ping.exe"];

    let tag = "demo-tag".to_string();
    println!("elevated: {:?}", is_elevated().map_err(|e| e.to_string()));

    let mut child = Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .env("NR_AC_TAG", &tag)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    println!(
        "NR_AC_TAG env: {:?}",
        process_env_var(child.id(), "NR_AC_TAG").map_err(|e| e.to_string())
    );

    let original_pid = child.id();

    println!("[full scan, no filter]");
    let started = Instant::now();
    match find_processes_by_env_tag("NR_AC_TAG", &tag, None) {
        Ok(scan) => println!(
            "  inspected: {}, skipped (unreadable): {}, matches: {:?}, took {:?}",
            scan.inspected,
            scan.skipped,
            scan.matches,
            started.elapsed()
        ),
        Err(e) => println!("  scan failed: {e}"),
    }

    println!("[recovery by tag, image filter {IMAGE_FILTER:?}]");
    let started = Instant::now();
    match find_processes_by_env_tag("NR_AC_TAG", &tag, Some(IMAGE_FILTER)) {
        Ok(scan) => {
            println!(
                "  inspected: {}, skipped (unreadable): {}, filtered out unopened: {}, took {:?}",
                scan.inspected,
                scan.skipped,
                scan.filtered_out,
                started.elapsed()
            );
            println!(
                "  matches: {:?} (original child: {original_pid})",
                scan.matches
            );
            for pid in scan.matches {
                let adopted = match AdoptedProcess::open(pid) {
                    Ok(adopted) => adopted,
                    Err(e) => {
                        println!("  pid {pid}: {e}");
                        continue;
                    }
                };
                println!(
                    "  pid {pid}: alive={:?}, start_time={:?}",
                    adopted.is_alive().map_err(|e| e.to_string()),
                    adopted.start_time().map_err(|e| e.to_string())
                );
                println!("  kill: {:?}", adopted.kill().map_err(|e| e.to_string()));
                println!(
                    "  exited within 5s: {}, alive={:?}",
                    adopted.wait(Duration::from_secs(5)),
                    adopted.is_alive().map_err(|e| e.to_string())
                );
            }
        }
        Err(e) => println!("  scan failed: {e}"),
    }
    let _ = child.kill();
    let _ = child.wait();
}
