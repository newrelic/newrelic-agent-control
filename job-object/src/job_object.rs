//! Windows Job Object wrapper for terminating a process and its descendants as a group.

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use tracing::{error, warn};
use windows::Wdk::System::SystemServices::NtMakeTemporaryObject;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GetLastError, HANDLE,
    HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACCESS_DENIED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION,
    AclSizeInformation, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
    GetAclInformation, GetLengthSid, GetTokenInformation, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser, WELL_KNOWN_SID_TYPE,
    WinLocalSystemSid,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, OpenJobObjectW,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PCWSTR};

const JOB_OBJECT_QUERY: u32 = 0x0004;
const READ_CONTROL: u32 = 0x0002_0000;
const DELETE: u32 = 0x0001_0000;
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;
const SECURITY_MAX_SID_SIZE: usize = 68;

/// Error produced by a Windows Job Object operation.
#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct JobObjectError(String);

/// Represents a Windows Job Object used to manage and control a group of processes.
/// When the Job Object is killed or dropped, all associated processes are terminated.
pub struct JobObject {
    handle: HANDLE,
}

impl JobObject {
    /// Creates a new JobObject with the "kill on job close" configuration.
    pub fn new() -> Result<Self, JobObjectError> {
        Self::create(PCWSTR::null())
    }

    /// Creates a new named JobObject, failing if an object with that name already exists, so a
    /// pre-created object is never silently reused.
    pub fn new_named(name: &str) -> Result<Self, JobObjectError> {
        let name = HSTRING::from(name);
        Self::create(PCWSTR(name.as_ptr()))
    }

    fn create(name: PCWSTR) -> Result<Self, JobObjectError> {
        unsafe {
            let handle = CreateJobObjectW(None, name)
                .map_err(|e| JobObjectError(format!("creating JobObject: {e}")))?;
            // CreateJobObjectW succeeds and returns the existing object when the name is taken.
            if !name.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return Err(JobObjectError("JobObject name already exists".to_string()));
            }

            // Set the JobObject to kill all associated processes when the JobObject is closed.
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| JobObjectError(format!("setting JobObject information: {e}")))?;

            Ok(Self { handle })
        }
    }

    /// Opens the existing JobObject `name` with `JOB_OBJECT_QUERY | READ_CONTROL` and returns it
    /// only if its owner and DACL are trusted.
    ///
    /// Returns `Ok(None)` when no such object exists, or when it is rejected. On rejection no
    /// process is adopted and a best-effort `NtMakeTemporaryObject` is attempted so the caller can
    /// create a fresh object with [`JobObject::new_named`]. The name is only released once every
    /// handle to the object is closed, so the fresh creation can still fail if another process
    /// holds one.
    ///
    /// The returned handle only has query access: it cannot assign processes or terminate.
    pub fn open_trusted(name: &str) -> Result<Option<Self>, JobObjectError> {
        let wide = HSTRING::from(name);
        let handle = match unsafe {
            OpenJobObjectW(
                JOB_OBJECT_QUERY | READ_CONTROL,
                false,
                PCWSTR(wide.as_ptr()),
            )
        } {
            Ok(handle) => handle,
            Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(None),
            Err(e) => return Err(JobObjectError(format!("opening JobObject: {e}"))),
        };

        match check_trust(handle) {
            Ok(Trust::Trusted) => Ok(Some(Self { handle })),
            Ok(Trust::Untrusted(reason)) => {
                warn!(%name, %reason, "Rejecting existing JobObject");
                unsafe {
                    let _ = CloseHandle(handle);
                }
                make_temporary(&wide);
                Ok(None)
            }
            Err(e) => {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                Err(e)
            }
        }
    }

    /// Assigns the given process to this JobObject. The process will be terminated when the JobObject is closed.
    pub fn assign_process(&self, process: &Child) -> Result<(), JobObjectError> {
        unsafe {
            let process_handle = HANDLE(process.as_raw_handle());
            AssignProcessToJobObject(self.handle, process_handle)
        }
        .map_err(|e| JobObjectError(format!("assigning process to JobObject: {e}")))
    }

    /// Terminates all processes in the JobObject, consuming it to prevent double-kills.
    pub fn kill(self) -> Result<(), JobObjectError> {
        unsafe { TerminateJobObject(self.handle, 0) }
            .map_err(|e| JobObjectError(format!("terminating JobObject: {e}")))
    }
}

/// Ensure the JobObject is killed when dropped.
impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            let _ = TerminateJobObject(self.handle, 0)
                .inspect_err(|err| error!(%err,"Fail to kill a JobObject"));
        }
    }
}

enum Trust {
    Trusted,
    Untrusted(String),
}

/// Trusts the object only if its owner is AC's own SID and every ACE in its DACL targets either
/// AC's SID or SYSTEM, matching the descriptor set at creation (`O:<AC-SID>D:P(A;;GA;;;SY)(A;;GA;;;<AC-SID>)`).
/// A missing DACL (full access for everyone) and any ACE type other than plain allow/deny are
/// rejected.
fn check_trust(handle: HANDLE) -> Result<Trust, JobObjectError> {
    let err = |what: &str, e: windows::core::Error| JobObjectError(format!("{what}: {e}"));

    // The only two accounts allowed to own the job or appear in its DACL: the account AC runs as
    // and SYSTEM. SIDs are kept as raw bytes.
    let own_sid = current_user_sid().map_err(|e| err("reading current user SID", e))?;
    let system_sid =
        well_known_sid(WinLocalSystemSid).map_err(|e| err("building SYSTEM SID", e))?;
    // Helper function that reads the length from each SID itself, so only pointers are needed.
    let same_sid = |sid: PSID, expected: &[u8]| {
        unsafe { EqualSid(sid, PSID(expected.as_ptr() as *mut _)) }.is_ok()
    };

    // Ask Windows for the job's owner and DACL. `owner` and `dacl` point into `descriptor`, which
    // Windows allocates and we must free with `LocalFree` once done (see the end of the function).
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut descriptor),
        )
    };
    // Failing to read the security info is an error, not a rejection: we can't judge the job.
    if status != ERROR_SUCCESS {
        return Err(JobObjectError(format!(
            "reading JobObject security info: {}",
            windows::core::Error::from(status.to_hresult())
        )));
    }

    // Computed in a closure so that every early return still reaches the `LocalFree` below.
    let verdict = (|| {
        // Job must be owned by AC itself.
        if owner.0.is_null() || !same_sid(owner, &own_sid) {
            return Ok(Trust::Untrusted(
                "owner is not the current user".to_string(),
            ));
        }
        // A null DACL means everyone has full access to the job.
        if dacl.is_null() {
            return Ok(Trust::Untrusted("missing DACL".to_string()));
        }
        // Read how many entries (ACEs) the DACL has.
        let mut info = ACL_SIZE_INFORMATION::default();
        unsafe {
            GetAclInformation(
                dacl,
                &mut info as *mut _ as *mut _,
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        }
        .map_err(|e| err("reading DACL information", e))?;

        // Every entry, allow or deny, must name AC or SYSTEM. The rights it grants are not
        // compared: any entry for another account is a squatting indicator.
        for index in 0..info.AceCount {
            // Pointer to the entry inside the DACL's memory.
            let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
            unsafe { GetAce(dacl, index, &mut ace) }.map_err(|e| err("reading ACE", e))?;
            // The account this entry applies to; `None` for entry types we don't parse.
            let Some(trustee) = (unsafe { trustee_sid(ace) }) else {
                return Ok(Trust::Untrusted(format!(
                    "ACE {index} is neither an allow nor a deny ACE"
                )));
            };
            // Reject the job if the entry names anyone other than AC or SYSTEM.
            let is_expected_trustee = same_sid(trustee, &own_sid) || same_sid(trustee, &system_sid);
            if !is_expected_trustee {
                return Ok(Trust::Untrusted(format!(
                    "ACE {index} targets an unexpected SID"
                )));
            }
        }
        Ok(Trust::Trusted)
    })();

    // Release the descriptor; `owner` and `dacl` must not be used after this point.
    unsafe {
        LocalFree(Some(HLOCAL(descriptor.0)));
    }
    verdict
}

/// Returns the SID of the account an ACE applies to, or `None` if it is not a plain allow or
/// deny ACE (the only layouts handled here).
///
/// We then get the sid by passing a pointer to the start of the SID in the ACE
///    byte 0                          byte 4         byte 8
///    ┌───────────────────────────────┬──────────────┬────────────────────────────┐
///    │ header (type, flags, size)    │ access mask  │ SID ... (variable length)  │
///    └───────────────────────────────┴──────────────┴────────────────────────────┘
///                                                    ▲
///                                                    └── SidStart
///
/// # Safety
/// `ace` must be null or a pointer returned by `GetAce` for an ACL that is still alive.
unsafe fn trustee_sid(ace: *mut core::ffi::c_void) -> Option<PSID> {
    if ace.is_null() {
        return None;
    }
    // Every ACE starts with a header that says what kind of ACE it is.
    let ace_type = unsafe { (*(ace as *const ACE_HEADER)).AceType };
    let sid_start = match ace_type {
        ACCESS_ALLOWED_ACE_TYPE => unsafe { &raw mut (*(ace as *mut ACCESS_ALLOWED_ACE)).SidStart },
        ACCESS_DENIED_ACE_TYPE => unsafe { &raw mut (*(ace as *mut ACCESS_DENIED_ACE)).SidStart },
        _ => return None,
    };
    Some(PSID(sid_start.cast()))
}

fn well_known_sid(kind: WELL_KNOWN_SID_TYPE) -> windows::core::Result<Vec<u8>> {
    let mut buf = vec![0u8; SECURITY_MAX_SID_SIZE];
    let mut size = buf.len() as u32;
    unsafe { CreateWellKnownSid(kind, None, Some(PSID(buf.as_mut_ptr().cast())), &mut size)? };
    buf.truncate(size as usize);
    Ok(buf)
}

fn current_user_sid() -> windows::core::Result<Vec<u8>> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let result = (|| {
            let mut len = 0u32;
            // The first call only reports the required size and is expected to fail.
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            // u64 elements keep the buffer aligned for TOKEN_USER.
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                len,
                &mut len,
            )?;
            let sid = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
            let sid_len = GetLengthSid(sid) as usize;
            Ok(std::slice::from_raw_parts(sid.0 as *const u8, sid_len).to_vec())
        })();
        let _ = CloseHandle(token);
        result
    }
}

/// Marks the rejected JobObject `name` as temporary, so that if it was permanent its name is
/// released once the last handle to it closes, letting a fresh job be created under that name.
///
/// The object is reopened by name with `DELETE` access, which `NtMakeTemporaryObject` requires.
/// This does not terminate the job's processes or destroy the object while other handles remain
/// open. Best effort: on failure a warning is logged and the name stays taken.
fn make_temporary(name: &HSTRING) {
    unsafe {
        let handle = match OpenJobObjectW(DELETE, false, PCWSTR(name.as_ptr())) {
            Ok(handle) => handle,
            Err(err) => {
                warn!(%err, "Cannot open rejected JobObject with DELETE access");
                return;
            }
        };
        let status = NtMakeTemporaryObject(handle);
        if status.is_err() {
            warn!(
                status = status.0,
                "NtMakeTemporaryObject failed on rejected JobObject"
            );
        }
        let _ = CloseHandle(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    fn test_job_object_kills_process() {
        let job = JobObject::new().expect("Failed to create JobObject");
        let mut child = Command::new("cmd")
            .args(["/C", "timeout", "/T", "15"])
            .spawn()
            .expect("Failed to spawn process");

        job.assign_process(&child)
            .expect("Failed to assign process to JobObject");

        job.kill().unwrap();
        let exited = (0..100).any(|_| {
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
            false
        });
        assert!(exited, "Failed to wait on process");
    }

    use windows::Win32::Foundation::HLOCAL;
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::core::PWSTR;

    /// Closes the creator's handle at the end of the test. The job is not permanent, so this also
    /// removes its name.
    struct Created(HANDLE);

    impl Drop for Created {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn unique_name(tag: &str) -> String {
        format!("Local\\ac-test-{tag}-{}", std::process::id())
    }

    fn own_sid_string() -> String {
        let sid = current_user_sid().unwrap();
        let mut out = PWSTR::null();
        unsafe {
            ConvertSidToStringSidW(PSID(sid.as_ptr() as *mut _), &mut out).unwrap();
            let s = out.to_string().unwrap();
            LocalFree(Some(HLOCAL(out.0 as *mut _)));
            s
        }
    }

    /// Creates a named job whose security descriptor is built from `sddl`.
    fn create_with_sddl(name: &str, sddl: &str) -> Created {
        let sddl = HSTRING::from(sddl);
        let name = HSTRING::from(name);
        unsafe {
            let mut sd = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
            .expect("parse SDDL");
            let attrs = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            };
            let handle = CreateJobObjectW(Some(&attrs), PCWSTR(name.as_ptr()));
            LocalFree(Some(HLOCAL(sd.0)));
            Created(handle.expect("create job with SDDL"))
        }
    }

    /// The descriptor AC is expected to set: owner is AC, only AC and SYSTEM have access.
    fn expected_sddl() -> String {
        let me = own_sid_string();
        format!("O:{me}D:P(A;;GA;;;SY)(A;;GA;;;{me})")
    }

    #[test]
    fn test_open_trusted_missing_returns_none() {
        let name = unique_name("missing");
        assert!(JobObject::open_trusted(&name).unwrap().is_none());
    }

    #[test]
    fn test_open_trusted_accepts_expected_descriptor() {
        let name = unique_name("accept");
        let _job = create_with_sddl(&name, &expected_sddl());
        assert!(JobObject::open_trusted(&name).unwrap().is_some());
    }

    #[test]
    fn test_open_trusted_rejects_extra_allow_ace() {
        let name = unique_name("extra-allow");
        // Read-only access for Everyone is still an unexpected trustee.
        let sddl = format!("{}(A;;GR;;;WD)", expected_sddl());
        let _job = create_with_sddl(&name, &sddl);
        assert!(JobObject::open_trusted(&name).unwrap().is_none());
    }

    #[test]
    fn test_open_trusted_rejects_extra_deny_ace() {
        let name = unique_name("extra-deny");
        let me = own_sid_string();
        let sddl = format!("O:{me}D:P(D;;GA;;;BG)(A;;GA;;;SY)(A;;GA;;;{me})");
        let _job = create_with_sddl(&name, &sddl);
        assert!(JobObject::open_trusted(&name).unwrap().is_none());
    }

    #[test]
    fn test_open_trusted_rejects_null_dacl() {
        let name = unique_name("null-dacl");
        let me = own_sid_string();
        let _job = create_with_sddl(&name, &format!("O:{me}D:NO_ACCESS_CONTROL"));
        assert!(JobObject::open_trusted(&name).unwrap().is_none());
    }

    #[test]
    fn test_rejected_job_keeps_name_while_creator_holds_handle() {
        let name = unique_name("name-held");
        let sddl = format!("{}(A;;GR;;;WD)", expected_sddl());
        let _job = create_with_sddl(&name, &sddl);
        assert!(JobObject::open_trusted(&name).unwrap().is_none());
        // The hostile creator still holds a handle, so a fresh job cannot take the name.
        assert!(JobObject::new_named(&name).is_err());
    }

    #[test]
    fn test_new_named_rejects_duplicates() {
        let name = unique_name("dup");
        let _first = JobObject::new_named(&name).expect("create named job");
        assert!(JobObject::new_named(&name).is_err());
    }

    #[test]
    fn test_new_named_succeeds_after_previous_handle_closed() {
        let name = unique_name("reuse");
        drop(create_with_sddl(&name, &expected_sddl()));
        assert!(JobObject::new_named(&name).is_ok());
    }

    #[test]
    fn test_open_trusted_accepts_dacl_with_only_own_sid() {
        let name = unique_name("own-only");
        let me = own_sid_string();
        let _job = create_with_sddl(&name, &format!("O:{me}D:P(A;;GA;;;{me})"));
        assert!(JobObject::open_trusted(&name).unwrap().is_some());
    }

    // When the DACL does not grant AC `JOB_OBJECT_QUERY`, the open itself fails, so the job is
    // reported as an error rather than rejected. These assume AC does not run as SYSTEM.
    #[test]
    fn test_open_trusted_errors_when_dacl_denies_query() {
        let name = unique_name("system-only");
        let me = own_sid_string();
        let _job = create_with_sddl(&name, &format!("O:{me}D:P(A;;GA;;;SY)"));
        assert!(JobObject::open_trusted(&name).is_err());
    }

    #[test]
    fn test_open_trusted_errors_on_empty_dacl() {
        let name = unique_name("empty-dacl");
        let me = own_sid_string();
        let _job = create_with_sddl(&name, &format!("O:{me}D:P"));
        assert!(JobObject::open_trusted(&name).is_err());
    }

    /// Lays out an ACE the way `GetAce` returns it: header (type, flags, size), access mask, SID.
    /// The buffer is `u64`s to keep it aligned.
    fn build_ace(ace_type: u8, sid: &[u8]) -> Vec<u64> {
        let size = 8 + sid.len();
        let mut bytes = vec![0u8; size];
        bytes[0] = ace_type;
        bytes[2..4].copy_from_slice(&(size as u16).to_le_bytes());
        bytes[8..].copy_from_slice(sid);
        let mut buf = vec![0u64; size.div_ceil(8)];
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.as_mut_ptr() as *mut u8, size);
        }
        buf
    }

    #[test]
    fn test_trustee_sid_returns_sid_for_allow_and_deny_aces() {
        let sid = well_known_sid(WinLocalSystemSid).unwrap();
        for ace_type in [ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE] {
            let mut ace = build_ace(ace_type, &sid);
            let found = unsafe { trustee_sid(ace.as_mut_ptr().cast()) }
                .unwrap_or_else(|| panic!("ACE type {ace_type} should be supported"));
            assert!(unsafe { EqualSid(found, PSID(sid.as_ptr() as *mut _)) }.is_ok());
        }
    }

    #[test]
    fn test_trustee_sid_rejects_null_pointer() {
        assert!(unsafe { trustee_sid(std::ptr::null_mut()) }.is_none());
    }

    #[test]
    fn test_trustee_sid_rejects_other_ace_types() {
        let sid = well_known_sid(WinLocalSystemSid).unwrap();
        // 2: system audit, 5: access allowed object, 9: access allowed callback.
        for ace_type in [2u8, 5, 9] {
            let mut ace = build_ace(ace_type, &sid);
            assert!(unsafe { trustee_sid(ace.as_mut_ptr().cast()) }.is_none());
        }
    }
}
