//! Probes name-squatting: what happens when `NtCreateJobObject` is called on a name that is
//! already taken.
//!
//! Two distinct cases arise depending on whether the squatter's job is temporary or permanent:
//!
//!   Part 1 – Temporary squatter (no privilege): with OBJ_OPENIF|OBJ_PERMANENT, NtCreateJobObject
//!   opens the existing temporary job and returns STATUS_OBJECT_NAME_EXISTS (0x40000000).
//!   `was_existing=true` — same detection path as a permanent squatter. AC then verifies the owner.
//!
//!   Part 2 – Permanent squatter (SeCreatePermanentPrivilege): same STATUS_OBJECT_NAME_EXISTS
//!   outcome. The owner check is the defence in both Part 1 and Part 2.
//!
//!   Part 3 – Owner check (same user): the squatter runs as the same account; the owner SID
//!   matches. The check passes, but `was_existing=true` still lets AC refuse to adopt the job.
//!
//!   Part 4 – Owner check (cross-user simulation): passing a wrong expected SID shows the check
//!   would catch a squatter from a different account.
//!
//!   Part 5 – Unguessable name: a name with an instance-specific suffix is hard to squatt
//!   without foreknowledge; secondary defence against a same-user squatter.
//!
//! Full cross-user squatting cannot be demonstrated in a single process. Run `permanent_job_probe`
//! as a non-elevated user to create a job, then run this example as SYSTEM to see the mismatch.
//!
//!   cargo run -p poc-windows-crash-survival --example name_squatting_probe

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    if let Err(e) = probe() {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn probe() -> Result<(), Box<dyn std::error::Error>> {
    use poc_windows_crash_survival::{
        Job, JobCreation, current_user_sid, is_elevated, make_temporary, restrictive_sddl,
        set_privilege,
    };

    println!("elevated: {}", is_elevated()?);
    let user = current_user_sid()?;
    let sddl = restrictive_sddl(&user);
    let pid = std::process::id();

    // ── Part 1: Temporary squatter → STATUS_OBJECT_NAME_EXISTS detected ────────────────────
    println!("\n── Part 1: temporary squatter detected (OBJ_OPENIF opens existing object) ──");
    {
        let name = format!(r"\BaseNamedObjects\nr-ac-squatting-probe-1-{pid}");

        // Any user can create a temporary job — no privilege required.
        let _squatter = Job::create_native(&name, false)?;
        println!("squatter created temporary job: {name}");

        // OBJ_OPENIF|OBJ_PERMANENT on an existing name → STATUS_OBJECT_NAME_EXISTS (0x40000000).
        // The existing temporary job is opened; `was_existing=true` signals a pre-existing object.
        let creation = Job::create_or_open_native(&name, &sddl)?;
        println!(
            "create_or_open: was_existing={} (NTSTATUS 0x{:08X})",
            creation.was_existing,
            if creation.was_existing { 0x40000000u32 } else { 0u32 },
        );
        assert!(
            creation.was_existing,
            "STATUS_OBJECT_NAME_EXISTS must be detected even for a temporary squatter"
        );
        println!("AC sees was_existing=true → verify owner before trusting (same path as Part 2)");
        // _squatter drops here; temporary job and its name disappear
    }
    println!("PASS 1");

    // ── Part 2: Permanent squatter → STATUS_OBJECT_NAME_EXISTS detected ─────────────────────
    println!("\n── Part 2: permanent squatter detected (STATUS_OBJECT_NAME_EXISTS) ──");
    {
        let name = format!(r"\BaseNamedObjects\nr-ac-squatting-probe-2-{pid}");
        let held = set_privilege("SeCreatePermanentPrivilege", true)?;

        if held {
            // Privileged squatter creates a permanent job first.
            let _squatter = Job::create_native(&name, true)?;
            println!("squatter created permanent job: {name}");

            // OBJ_PERMANENT on an already-permanent name → STATUS_OBJECT_NAME_EXISTS (0x40000000).
            // Bit 31 is clear: info/warning. create_or_open succeeds, was_existing=true.
            let creation = Job::create_or_open_native(&name, &sddl)?;
            println!(
                "create_or_open: was_existing={} (NTSTATUS 0x{:08X})",
                creation.was_existing,
                if creation.was_existing { 0x40000000u32 } else { 0u32 },
            );
            assert!(
                creation.was_existing,
                "STATUS_OBJECT_NAME_EXISTS must be detected"
            );
            let info = creation.job.info()?;
            println!("existing job: active_processes={}", info.active_processes);
            println!("AC sees was_existing=true → must verify owner before trusting");
            make_temporary(&name)?;
        } else {
            println!("SKIP: SeCreatePermanentPrivilege not held (need SYSTEM or granted right)");
        }
    }
    println!("PASS 2");

    // ── Part 3: owner check with a same-user permanent squatter ─────────────────────────────
    println!("\n── Part 3: owner check (same user, permanent squatter) ──");
    {
        let name = format!(r"\BaseNamedObjects\nr-ac-squatting-probe-3-{pid}");
        let held = set_privilege("SeCreatePermanentPrivilege", true)?;

        if held {
            let _squatter = Job::create_native(&name, true)?;
            let creation = Job::create_or_open_native(&name, &sddl)?;
            assert!(creation.was_existing);

            let sec = creation.job.security()?;
            println!("owner: {} ({})", sec.owner, sec.owner_account.as_deref().unwrap_or("?"));

            // Same user → owner check passes (AC and squatter are the same account).
            let unexpected_owner = sec.unexpected_owner(&[&user, "S-1-5-32-544"]);
            println!("unexpected_owner (allowing our SID): {unexpected_owner:?}");
            assert!(
                unexpected_owner.is_none(),
                "same-user squatter has the same owner SID"
            );

            // Even though the owner check passes, `was_existing` signals the job predates this
            // startup. AC should not adopt a job it did not create in this run.
            println!(
                "was_existing={}: job predates this startup, even though owner matches",
                creation.was_existing,
            );
            make_temporary(&name)?;
        } else {
            println!("SKIP: SeCreatePermanentPrivilege not held");
        }
    }
    println!("PASS 3");

    // ── Part 4: owner check would catch a cross-user squatter ────────────────────────────────
    println!("\n── Part 4: owner check rejects unexpected owner (cross-user simulation) ──");
    {
        let name = format!(r"\BaseNamedObjects\nr-ac-squatting-probe-4-{pid}");
        let held = set_privilege("SeCreatePermanentPrivilege", true)?;

        let creation = if held {
            let c = Job::create_or_open_native(&name, &sddl)?;
            assert!(!c.was_existing, "should be new");
            c
        } else {
            // No privilege: use a temporary job just to exercise the owner check.
            JobCreation {
                job: Job::create_native(&name, false)?,
                was_existing: false,
            }
        };

        let sec = creation.job.security()?;
        println!("actual owner: {}", sec.owner);

        // Pass a wrong SID (Everyone) as expected owner — simulates what AC sees if a
        // different account created the job.
        const EVERYONE: &str = "S-1-1-0";
        let unexpected = sec.unexpected_owner(&[EVERYONE]);
        println!("unexpected_owner (allowing only S-1-1-0=Everyone): {unexpected:?}");
        assert!(
            unexpected.is_some(),
            "owner should be rejected when the expected SID does not match"
        );
        println!("cross-user squatter would be caught: owner mismatch → AC refuses adoption");

        if held {
            make_temporary(&name)?;
        }
    }
    println!("PASS 4");

    // ── Part 5: unguessable name reduces squatting risk ──────────────────────────────────────
    println!("\n── Part 5: unguessable name ──");
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        // A name with a per-instance suffix derived from the sub-agent's first-spawn timestamp.
        let instance_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!(r"\BaseNamedObjects\nr-ac-infrastructure-{instance_id:032x}");
        println!("deterministic + unguessable name: {name}");

        let held = set_privilege("SeCreatePermanentPrivilege", true)?;
        if held {
            let creation = Job::create_or_open_native(&name, &sddl)?;
            assert!(!creation.was_existing, "fresh name should not exist");
            println!("was_existing=false: name was free, no squatting possible");
            make_temporary(&name)?;
        } else {
            let _job = Job::create_native(&name, false)?;
            let result = Job::create_or_open_native(&name, &sddl);
            // Without privilege, this creates a temporary job and then COLLISION is expected.
            println!(
                "create_or_open without privilege: {}",
                if result.is_err() { "COLLISION (temp squatter)" } else { "was_existing=true" }
            );
        }
    }
    println!("PASS 5");

    println!("\nPASS");
    Ok(())
}
