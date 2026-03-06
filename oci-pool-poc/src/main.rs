//! PoC: demonstrates that regex Pool<meta::Cache> accumulates one permanent entry per distinct
//! OS thread that calls Reference::from_str(), even when threads are purely sequential.
//!
//! Each "cycle" mimics a `recreate_sub_agent()` event: the old "Subagent runtime" thread exits
//! and a new one is spawned. The new thread calls Reference::from_str() (via template_with /
//! assemble_agent in production), which checks out a Pool<meta::Cache> slot indexed by thread ID.
//! When the thread exits the cache is returned to the pool but NOT freed. A new thread with a
//! different ID may land in a different slot → new permanent ~5.87 MB allocation.

use std::str::FromStr;
// use oci_spec::distribution::Reference;
// use std::str::FromStr;
use std::thread;
use std::time::Duration;

use crate::oci_reference::OciReference;

mod oci_reference;

const CYCLES: usize = 80;
const SLEEP_MS: u64 = 300; // simulates download/process work

fn rss_kb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
        let pages: u64 = s
            .split_whitespace()
            .nth(1)
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        pages * 4 // assume 4 KB pages
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default();
        String::from_utf8_lossy(&out)
            .trim()
            .parse::<u64>()
            .unwrap_or(0)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

fn main() {
    println!("Spawning {CYCLES} sequential threads, each calling Reference::from_str() once.");
    println!(
        "Each thread represents one 'Subagent runtime' cycle (old thread exits, new spawned).\n"
    );
    println!("{:>6}  {:>12}  thread_id", "cycle", "rss (KB)");

    let mut oci_refs = [
        "docker.io/newrelic/infrastructure-agent-artifacts:1.71.1",
        "docker.io/newrelic/nrdot-agent-artifacts:1.11.0",
    ]
    .into_iter()
    .cycle();

    for i in 0..CYCLES {
        let oci_ref = oci_refs.next().unwrap(); // cycle always ensures this is safe
        let handle = thread::Builder::new()
            .name(format!("subagent-runtime-{i}"))
            .spawn(move || {
                let tid = format!("{:?}", thread::current().id());
                // Mimics: assemble_agent() → Oci::template_with() → Reference::from_str()
                // This checks out a Pool<meta::Cache> slot keyed to this thread's ID.
                let reference = OciReference::from_str(oci_ref).expect("valid reference");
                let _ = reference; // keep alive past the regex call

                // Simulate download / supervisor startup work
                thread::sleep(Duration::from_millis(SLEEP_MS));
                tid
            })
            .expect("spawn failed");

        let tid = handle.join().expect("thread panicked");
        let rss = rss_kb();
        println!("{:>6}  {:>12}  {}", i, rss, tid);
    }

    println!("\nDone.");
    println!("  RSS grew by ~5-6 MB per new pool slot → hypothesis confirmed.");
    println!(
        "  RSS stayed flat            → pool slots are being reused (thread_id % pool_size collision)."
    );
}
