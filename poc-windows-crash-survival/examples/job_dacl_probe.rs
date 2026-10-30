//! Checks the DACL of a permanent named job right after creation, and shows how to create one
//! with a restrictive security descriptor instead of the default one.
//!
//!   cargo run -p poc-windows-crash-survival --example job_dacl_probe -- [enable|disable|leave]
//!
//! The optional argument controls `SeCreatePermanentPrivilege` as in `permanent_job_probe`.
//!
//! For each namespace it creates two permanent jobs, one with the default DACL (no security
//! descriptor passed) and one with an explicit SDDL, and for each one:
//!   1. prints owner, SDDL and every ACE read from the job handle,
//!   2. closes the handle, reopens the job by name with `READ_CONTROL` and checks the DACL is the
//!      same one (it is stored in the object, not in the handle),
//!   3. audits the job: an owner other than the user or Administrators, and any allow ACE for a
//!      trustee other than the user and `SYSTEM`, are reported.
//!
//! Permanent jobs are made temporary again before exiting. When the permanent create is refused
//! (no privilege) the same steps run on a temporary job, since the DACL logic is identical.

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::{
        Job, ObjectSecurity, READ_CONTROL, current_user_sid, is_elevated, make_temporary,
        restrictive_sddl, set_privilege,
    };
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::GetCurrentProcessId;

    const SYSTEM_SID: &str = "S-1-5-18";
    // Objects created by an elevated token or by SYSTEM are owned by this group.
    const ADMINISTRATORS_SID: &str = "S-1-5-32-544";

    let mut session = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.unwrap();
    println!(
        "elevated: {:?}, session: {session}",
        is_elevated().map_err(|e| e.to_string())
    );
    let user = current_user_sid().unwrap();
    println!("current user SID: {user}");

    match std::env::args().nth(1).as_deref().unwrap_or("enable") {
        "leave" => println!("SeCreatePermanentPrivilege: token left as issued"),
        mode @ ("enable" | "disable") => println!(
            "SeCreatePermanentPrivilege held, after {mode}: {:?}",
            set_privilege("SeCreatePermanentPrivilege", mode == "enable")
                .map_err(|e| e.to_string())
        ),
        other => {
            eprintln!("unknown mode {other}, expected enable, disable or leave");
            std::process::exit(2);
        }
    }

    let print_security = |indent: &str, security: &ObjectSecurity| {
        println!(
            "{indent}owner: {} ({})",
            security.owner,
            security.owner_account.as_deref().unwrap_or("unresolved")
        );
        println!("{indent}SDDL:  {}", security.sddl);
        match &security.aces {
            None => println!("{indent}DACL:  NULL (no access control at all)"),
            Some(aces) => {
                for ace in aces {
                    println!(
                        "{indent}ACE:   {:?} {:#010X} {} ({})",
                        ace.kind,
                        ace.mask,
                        ace.sid,
                        ace.account.as_deref().unwrap_or("unresolved")
                    );
                }
            }
        }
        match security.unexpected_owner(&[&user, ADMINISTRATORS_SID]) {
            None => println!("{indent}audit: owner is the current user or Administrators"),
            Some(owner) => println!("{indent}audit: unexpected owner {owner}, not our job"),
        }
        let extra = security.unexpected_grants(&[&user, SYSTEM_SID]);
        if extra.is_empty() {
            println!("{indent}audit: only the current user and SYSTEM are granted access");
        } else {
            println!("{indent}audit: also granted to: {extra:?}");
        }
    };

    let mut targets = vec![(
        "global namespace",
        r"\BaseNamedObjects\nr-ac-dacl-probe".to_string(),
    )];
    if session != 0 {
        targets.push((
            "session namespace",
            format!(r"\Sessions\{session}\BaseNamedObjects\nr-ac-dacl-probe"),
        ));
    }

    let restrictive = restrictive_sddl(&user);
    // (label, SDDL, None = default DACL)
    let variants = [
        ("default DACL", None),
        ("restrictive DACL", Some(restrictive.as_str())),
    ];

    for (label, nt_path) in targets {
        println!("--- {label}: {nt_path}");
        for (variant, sddl) in variants {
            println!("  [{variant}]");
            if let Some(sddl) = sddl {
                println!("    requested SDDL: {sddl}");
            }
            let create = |permanent: bool| match sddl {
                Some(sddl) => Job::create_native_with_sddl(&nt_path, permanent, sddl),
                None => Job::create_native(&nt_path, permanent),
            };
            let (job, permanent) = match create(true) {
                Ok(job) => (job, true),
                Err(e) => {
                    println!("    permanent create failed ({e}), using a temporary job");
                    match create(false) {
                        Ok(job) => (job, false),
                        Err(e) => {
                            println!("    temporary create failed: {e}");
                            continue;
                        }
                    }
                }
            };

            println!("    after creation (permanent: {permanent}), read from the handle:");
            match job.security() {
                Ok(security) => print_security("      ", &security),
                Err(e) => println!("      cannot read: {e}"),
            }

            if permanent {
                drop(job);
                println!("    last handle closed, reopened by name with READ_CONTROL:");
                match Job::open_native(&nt_path, READ_CONTROL).and_then(|job| job.security()) {
                    Ok(security) => print_security("      ", &security),
                    Err(e) => println!("      cannot read: {e}"),
                }
                println!("    cleanup: {:?}", make_temporary(&nt_path));
            }
        }
    }
}
