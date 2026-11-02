//! End-to-end crash survival check using a per-sub-agent cgroup as the only record.
//!
//! Needs cgroup v2 and write access to AC's own cgroup (root, or a delegated subtree). If that is
//! not available the example prints `SKIP` and exits 0. In a container use `--privileged`.
//!
//!   cargo run -p poc-linux-crash-survival --example crash_survival_cgroup
//!
//! The example re-executes itself as a "supervisor" that creates `nr-ac-poc-<id>` and spawns a
//! sub-agent inside it that SCRUBS ITS ENVIRONMENT (`env -i`) and forks two sleepers, then aborts.
//! The parent plays the restarted AC: it finds the cgroup by name, lists the members straight from
//! the kernel (env tag and bookkeeping could not), kills them atomically and removes the cgroup.

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

/// The cgroup name is derived from the agent and exec ids, so AC needs no stored state.
#[cfg(target_os = "linux")]
fn cgroup_name(id: &str) -> String {
    format!("nr-ac-poc-{id}")
}

/// Creates the cgroup, spawns the sub-agent into it and crashes without any cleanup.
#[cfg(target_os = "linux")]
fn supervisor(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    use poc_linux_crash_survival::cgroup::{Cgroup, own_cgroup_dir, spawn_in_cgroup};
    use std::process::Command;

    let cgroup = Cgroup::create(&own_cgroup_dir()?, &cgroup_name(id))?;
    // `env -i` wipes the environment (the env tag would be lost) and the shell forks two sleepers.
    let mut cmd = Command::new("env");
    cmd.args(["-i", "/bin/sh", "-c", "sleep 300 & sleep 300 & wait"]);
    spawn_in_cgroup(&mut cmd, &cgroup)?;

    // Abort skips destructors; the kernel just drops our resources, as in a real crash.
    std::process::abort();
}

/// Acts as the restarted AC. A failed `assert!` panics, which also exits with a non-zero status.
#[cfg(target_os = "linux")]
fn orchestrator() -> Result<(), Box<dyn std::error::Error>> {
    use poc_linux_crash_survival::cgroup::{Cgroup, own_cgroup_dir};
    use poc_linux_crash_survival::{is_root, wait_until};
    use std::process::Command;
    use std::time::Duration;

    println!("root: {}", is_root());

    // The sub-agent cgroups live under AC's own cgroup. Skip if we cannot create one.
    let parent = match own_cgroup_dir() {
        Ok(parent) => parent,
        Err(err) => return skip(&format!("{err}")),
    };
    let id = std::process::id().to_string();
    let probe = parent.join(format!("nr-ac-poc-probe-{id}"));
    if let Err(err) = std::fs::create_dir(&probe) {
        return skip(&format!(
            "cannot create a cgroup under {}: {err}",
            parent.display()
        ));
    }
    let _ = std::fs::remove_dir(&probe);

    // Step 1: run the supervisor and let it crash.
    let status = Command::new(std::env::current_exe()?)
        .arg("supervisor")
        .arg(&id)
        .status()?;
    assert!(!status.success(), "supervisor should have crashed");

    // Step 2: the restarted AC finds the cgroup by name. No scanning, no env, no stored state.
    let cgroup = Cgroup::open(&parent, &cgroup_name(&id))
        .ok_or("the named cgroup should persist after the supervisor died")?;
    let members = wait_until(Duration::from_secs(5), || {
        let members = cgroup.live_members();
        (members.len() == 3).then_some(members)
    })
    .ok_or_else(|| {
        format!(
            "expected the shell and 2 sleepers, found {:?}",
            cgroup.procs()
        )
    })?;
    println!("cgroup {} members: {members:?}", cgroup.path().display());

    // Step 3: kill everything atomically, then clean the cgroup up.
    cgroup.kill_all()?;
    wait_until(Duration::from_secs(5), || {
        cgroup.live_members().is_empty().then_some(())
    })
    .ok_or("the cgroup should have no live members after the kill")?;
    // Zombies keep the cgroup busy until reaped; a container without an init never reaps them.
    match cgroup.remove(Duration::from_secs(3)) {
        Ok(()) => println!("cgroup removed"),
        Err(err) => println!("WARN: cgroup not removed ({err}); run the container with --init"),
    }

    println!(
        "PASS: members found by cgroup name despite a scrubbed environment, killed atomically"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn skip(reason: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("SKIP: {reason}");
    Ok(())
}
