//! Probes whether a named job can outlive its last handle by creating it as a permanent kernel
//! object (`OBJ_PERMANENT`), and what that requires.
//!
//!   cargo run -p poc-windows-crash-survival --example permanent_job_probe -- [enable|disable|leave]
//!
//! The optional argument controls `SeCreatePermanentPrivilege` in the process token before the
//! permanent create: `enable` (default), `disable` (held but disabled, only meaningful if the
//! token holds it) or `leave` (use the token as it was issued).
//!
//! Run it as a non-elevated user and as `SYSTEM` to compare. For each namespace it creates a
//! temporary job (baseline) and a permanent one, closes every handle, and tries to open the job
//! by name again. A permanent job that was opened is made temporary again before exiting, so no
//! name leaks until the next reboot.

#[cfg(not(windows))]
fn main() {
    eprintln!("This PoC only runs on Windows");
}

#[cfg(windows)]
mod nt {
    use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows::Win32::Foundation::HANDLE;

    #[link(name = "ntdll", kind = "raw-dylib")]
    unsafe extern "system" {
        pub fn NtCreateJobObject(
            job_handle: *mut HANDLE,
            desired_access: u32,
            object_attributes: *const OBJECT_ATTRIBUTES,
        ) -> i32;
        pub fn NtMakeTemporaryObject(handle: HANDLE) -> i32;
    }
}

#[cfg(windows)]
fn main() {
    use poc_windows_crash_survival::{Job, is_elevated, set_privilege};
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, OBJ_CASE_INSENSITIVE, OBJ_PERMANENT, OBJECT_ATTRIBUTE_FLAGS,
        UNICODE_STRING,
    };
    use windows::Win32::System::JobObjects::{AssignProcessToJobObject, OpenJobObjectW};
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::SystemServices::JOB_OBJECT_QUERY;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::core::{HSTRING, PWSTR};

    const DELETE: u32 = 0x0001_0000;
    const JOB_OBJECT_ALL_ACCESS: u32 = 0x1F_001F;

    let mut session = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.unwrap();
    println!(
        "elevated: {:?}, session: {session}",
        is_elevated().map_err(|e| e.to_string())
    );

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

    // (label, NT path used to create, Win32 name used to open)
    let mut targets = vec![(
        "global namespace",
        r"\BaseNamedObjects\nr-ac-perm-probe".to_string(),
        r"Global\nr-ac-perm-probe".to_string(),
    )];
    if session != 0 {
        targets.push((
            "session namespace",
            format!(r"\Sessions\{session}\BaseNamedObjects\nr-ac-perm-probe"),
            r"Local\nr-ac-perm-probe".to_string(),
        ));
    }

    // Creates the named job via the native API so `OBJ_PERMANENT` can be requested.
    let create = |nt_path: &str, permanent: bool| -> Result<HANDLE, i32> {
        let mut wide: Vec<u16> = nt_path.encode_utf16().collect();
        let name = UNICODE_STRING {
            Length: (wide.len() * 2) as u16,
            MaximumLength: (wide.len() * 2) as u16,
            Buffer: PWSTR(wide.as_mut_ptr()),
        };
        let attrs = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            ObjectName: &name,
            Attributes: OBJECT_ATTRIBUTE_FLAGS(
                OBJ_CASE_INSENSITIVE.0 | if permanent { OBJ_PERMANENT.0 } else { 0 },
            ),
            ..Default::default()
        };
        let mut handle = HANDLE::default();
        let status = unsafe { nt::NtCreateJobObject(&mut handle, JOB_OBJECT_ALL_ACCESS, &attrs) };
        if status >= 0 { Ok(handle) } else { Err(status) }
    };

    for (label, nt_path, win32_name) in targets {
        println!("--- {label}: {nt_path}");

        for permanent in [false, true] {
            let kind = if permanent { "permanent" } else { "temporary" };
            let handle = match create(&nt_path, permanent) {
                Ok(handle) => handle,
                Err(status) => {
                    println!("  {kind}: create failed, NTSTATUS {:#010X}", status as u32);
                    continue;
                }
            };

            let mut child = Command::new("ping")
                .args(["-n", "60", "127.0.0.1"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let assigned =
                unsafe { AssignProcessToJobObject(handle, HANDLE(child.as_raw_handle())) };
            println!("  {kind}: created, assign: {assigned:?}");

            unsafe { CloseHandle(handle) }.unwrap();
            print!("  {kind}: last handle closed, open by name: ");
            match Job::open(&win32_name) {
                Ok(job) => println!("{:?}", job.info().map_err(|e| e.to_string())),
                Err(e) => println!("{e}"),
            }

            if permanent {
                // Undo the permanence: needs DELETE access on the handle.
                match unsafe {
                    OpenJobObjectW(
                        DELETE | JOB_OBJECT_QUERY,
                        false,
                        &HSTRING::from(&win32_name),
                    )
                } {
                    Ok(h) => {
                        let status = unsafe { nt::NtMakeTemporaryObject(h) };
                        unsafe { CloseHandle(h) }.unwrap();
                        println!(
                            "  permanent: made temporary, NTSTATUS {:#010X}",
                            status as u32
                        );
                        print!("  permanent: after cleanup, open by name: ");
                        match Job::open(&win32_name) {
                            Ok(_) => println!("still present"),
                            Err(e) => println!("{e}"),
                        }
                    }
                    Err(e) => println!("  permanent: cannot open for cleanup: {e}"),
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
