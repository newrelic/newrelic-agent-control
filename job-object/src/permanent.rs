//! Permanent named Job Objects: they keep their name after their last handle is closed, so the
//! processes they track can be found again by name.

use crate::job_handle::{JobHandle, JobObjectError};
use crate::job_name::JobName;
use std::ops::Deref;
use windows::Wdk::System::SystemServices::NtMakeTemporaryObject;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INVALID_HANDLE, GetLastError, HANDLE, HLOCAL,
    LocalFree, NTSTATUS,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SECURITY_MAX_SID_SIZE,
    TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::JobObjects::{CreateJobObjectW, OpenJobObjectW};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, Owned, PWSTR};

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    /// Makes a named object permanent. The `windows` crate exposes only its counterpart,
    /// `NtMakeTemporaryObject`.
    fn NtMakePermanentObject(handle: HANDLE) -> NTSTATUS;
}

/// Permanent named Job Object, without the "kill on job close" configuration. Dropping it only
/// closes the handle, whether the job was created or opened, so its processes keep running until
/// [PermanentJobObject::kill] is called.
pub struct PermanentJobObject(JobHandle);

impl PermanentJobObject {
    /// Creates the permanent JobObject `name`. A permanent JobObject keeps its name after its last
    /// handle is closed, until [PermanentJobObject::make_temporary] is called or the host reboots.
    ///
    /// Its security descriptor is set atomically at creation: the owner is the account running
    /// this process, and only that account and `SYSTEM` get access. It fails if an object with
    /// that name already exists, so a pre-created object is never reused, and if
    /// `SeCreatePermanentPrivilege` is not enabled in the process token.
    pub fn new(name: &JobName) -> Result<Self, JobObjectError> {
        let descriptor = SecurityDescriptor::for_current_user().map_err(|e| {
            JobObjectError(format!(
                "creating permanent JobObject {name}: building security descriptor: {e}"
            ))
        })?;

        let handle = create_new_named_job_object(name, &descriptor)
            .map_err(|e| JobObjectError(format!("creating a named JobObject {name}: {e}")))?;

        let status = unsafe { NtMakePermanentObject(*handle) };
        status.ok().map_err(|e| {
            JobObjectError(format!(
                "creating permanent JobObject {name}: making it permanent: {e}"
            ))
        })?;

        Ok(Self(JobHandle { handle }))
    }

    /// Opens the existing JobObject `name` with the `access` rights, returning `None` if it does
    /// not exist.
    ///
    /// Each operation needs its own right, documented in
    /// <https://learn.microsoft.com/en-us/windows/win32/procthread/job-object-security-and-access-rights>:
    /// - `JOB_OBJECT_QUERY` for [JobHandle::accounting] and [JobHandle::process_ids].
    /// - `JOB_OBJECT_SET_ATTRIBUTES` and `JOB_OBJECT_QUERY` for
    ///   [JobHandle::limit_active_processes], which reads the current limits first.
    /// - `JOB_OBJECT_ASSIGN_PROCESS` for [JobHandle::assign_process].
    /// - `JOB_OBJECT_TERMINATE` for [PermanentJobObject::kill].
    /// - `DELETE` for [PermanentJobObject::make_temporary].
    pub fn open(name: &JobName, access: u32) -> Result<Option<Self>, JobObjectError> {
        // Since we do not want to pass the handle down to child process we set false for binherithandle
        match unsafe { OpenJobObjectW(access, false, &HSTRING::from(name.as_ref())) } {
            // Safety: the handle was just returned and is owned here.
            Ok(handle) => Ok(Some(Self(JobHandle {
                handle: unsafe { Owned::new(handle) },
            }))),
            Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(None),
            Err(e) => Err(JobObjectError(format!("opening JobObject {name}: {e}"))),
        }
    }

    /// Turns a permanent JobObject into a temporary one: its name is removed once its last
    /// handle is closed, and the object is destroyed once its processes have exited too. It
    /// requires the `DELETE` right, which a JobObject created by [PermanentJobObject::new] has.
    ///
    /// `DELETE` is the standard right to delete an object, documented in
    /// <https://learn.microsoft.com/en-us/windows/win32/secauthz/standard-access-rights>, which
    /// `NtMakeTemporaryObject`
    /// (<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-zwmaketemporaryobject>)
    /// checks since a temporary object is deleted with its last handle. The `windows` crate only
    /// exposes it as a file access right, `windows::Win32::Storage::FileSystem::DELETE`.
    pub fn make_temporary(&self) -> Result<(), JobObjectError> {
        unsafe { NtMakeTemporaryObject(*self.handle) }
            .ok()
            .map_err(|e| JobObjectError(format!("making JobObject temporary: {e}")))
    }

    /// Terminates all processes in the JobObject, consuming it to prevent double-kills. It
    /// requires the `JOB_OBJECT_TERMINATE` right, which a JobObject created by
    /// [PermanentJobObject::new] has.
    pub fn kill(self) -> Result<(), JobObjectError> {
        self.terminate()
    }
}

impl Deref for PermanentJobObject {
    type Target = JobHandle;

    fn deref(&self) -> &JobHandle {
        &self.0
    }
}

/// Creates the job `name` with `descriptor`, failing if an object with that name already exists.
fn create_new_named_job_object(
    name: &JobName,
    descriptor: &SecurityDescriptor,
) -> Result<Owned<HANDLE>, String> {
    const NAME_TAKEN: &str = "name already exists";
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.as_ptr(),
        bInheritHandle: false.into(),
    };
    let created = unsafe { CreateJobObjectW(Some(&attributes), &HSTRING::from(name.as_ref())) };
    // `CreateJobObjectW` opens the job instead if the name exists, and only reports it here.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

    // Safety: the handle was just returned and is owned here.
    let handle = created
        .map(|handle| unsafe { Owned::new(handle) })
        .map_err(|e| {
            // An object of another type holds the name.
            if e.code() == ERROR_INVALID_HANDLE.to_hresult() {
                NAME_TAKEN.to_string()
            } else {
                e.to_string()
            }
        })?;
    if existed {
        // Dropping `handle` only closes it, so the processes of the existing job are unaffected.
        return Err(NAME_TAKEN.to_string());
    }
    Ok(handle)
}

/// A self-relative security descriptor.
///
/// It is set atomically at creation: setting it afterwards would leave a window in which another
/// process could open the job with the default, permissive, DACL.
struct SecurityDescriptor(LocalMemory);

impl SecurityDescriptor {
    /// Descriptor of permanent jobs: the owner is the account running this process, and only it
    /// and `SYSTEM` get access.
    fn for_current_user() -> windows::core::Result<Self> {
        Self::from_sddl(&permanent_job_sddl(&current_user_sid()?))
    }

    fn from_sddl(sddl: &str) -> windows::core::Result<Self> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }?;
        Ok(Self(LocalMemory(descriptor.0)))
    }

    /// Pointer to the descriptor, valid while `self` is alive.
    fn as_ptr(&self) -> *mut core::ffi::c_void {
        self.0.0
    }
}

/// Returns the SDDL of the descriptor of permanent jobs, owned by `user_sid`. `P` blocks inherited
/// ACEs.
fn permanent_job_sddl(user_sid: &str) -> String {
    format!("O:{user_sid}D:P(A;;GA;;;SY)(A;;GA;;;{user_sid})")
}

/// Returns the SID, in string form, of the account the current process runs as.
fn current_user_sid() -> windows::core::Result<String> {
    let mut token = Owned::<HANDLE>::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut *token) }?;

    // A `TOKEN_USER` followed by its SID, which is never longer than `SECURITY_MAX_SID_SIZE`.
    // `u64` elements keep the buffer aligned for `TOKEN_USER`.
    const BUFFER_SIZE: usize = size_of::<TOKEN_USER>() + SECURITY_MAX_SID_SIZE as usize;
    let mut buffer = [0u64; BUFFER_SIZE.div_ceil(8)];
    let mut len = 0;
    unsafe {
        GetTokenInformation(
            *token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size_of_val(&buffer) as u32,
            &mut len,
        )
    }?;
    // Safety: on success the buffer starts with a `TOKEN_USER` whose SID points inside it.
    let sid = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() }.User.Sid;

    let mut string_sid = PWSTR::null();
    unsafe { ConvertSidToStringSidW(sid, &mut string_sid) }?;
    let _guard = LocalMemory(string_sid.0.cast());
    Ok(String::from_utf16_lossy(unsafe { string_sid.as_wide() }))
}

/// Memory allocated by the system for the caller, released with `LocalFree` on drop.
struct LocalMemory(*mut core::ffi::c_void);

impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe { LocalFree(Some(HLOCAL(self.0))) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_handle::tests::{
        EXIT_TIMEOUT, exits_within, kill_and_wait, long_running_process,
    };
    use crate::{SE_CREATE_PERMANENT_PRIVILEGE, enable_privilege};
    use rstest::rstest;
    use std::any::Any;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
    };
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
    use windows::Win32::System::SystemServices::{
        JOB_OBJECT_ASSIGN_PROCESS, JOB_OBJECT_QUERY, JOB_OBJECT_SET_ATTRIBUTES,
        JOB_OBJECT_TERMINATE,
    };
    use windows::Win32::System::Threading::CreateEventW;

    /// The standard `DELETE` right, see [PermanentJobObject::make_temporary].
    const DELETE: u32 = 0x0001_0000;

    #[test]
    fn test_current_user_descriptor_parses() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "unexpected SID {sid}");
        SecurityDescriptor::for_current_user().unwrap();
    }

    #[test]
    fn test_invalid_sddl_fails() {
        assert!(SecurityDescriptor::from_sddl("not an SDDL").is_err());
    }

    #[test]
    fn test_open_missing_job_returns_none() {
        let name = test_job_name("missing");
        assert!(
            PermanentJobObject::open(&name, JOB_OBJECT_QUERY)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    #[ignore = "requires root"]
    fn test_permanent_job_outlives_its_handles_until_made_temporary_as_root() {
        let name = test_job_name("permanent");
        let _cleanup = PermanentName::new(&name);

        drop(PermanentJobObject::new(&name).unwrap());

        let reopened = PermanentJobObject::open(&name, DELETE)
            .unwrap()
            .expect("the permanent job should keep its name");
        reopened.make_temporary().unwrap();
        drop(reopened);
        assert!(
            PermanentJobObject::open(&name, JOB_OBJECT_QUERY)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    #[ignore = "requires root"]
    fn test_permanent_job_is_found_by_name_as_root() {
        let name = test_job_name("found");
        let _cleanup = PermanentName::new(&name);
        let job = PermanentJobObject::new(&name).unwrap();
        let child = long_running_process();
        job.assign_process(&child).unwrap();

        let opened = PermanentJobObject::open(&name, JOB_OBJECT_QUERY)
            .unwrap()
            .expect("the job should be found by name");

        assert_eq!(opened.process_ids().unwrap(), vec![child.id()]);
        kill_and_wait(&job, vec![child]);
    }

    #[test]
    #[ignore = "requires root"]
    fn test_dropping_the_last_handle_keeps_the_processes_running_as_root() {
        let name = test_job_name("drop");
        let _cleanup = PermanentName::new(&name);
        let job = PermanentJobObject::new(&name).unwrap();
        let mut child = long_running_process();
        job.assign_process(&child).unwrap();

        drop(job);

        assert!(
            !exits_within(&mut child, EXIT_TIMEOUT),
            "process was killed"
        );
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    #[ignore = "requires root"]
    fn test_opened_job_can_be_shut_down_safely_as_root() {
        let name = test_job_name("safe-shutdown");
        let _cleanup = PermanentName::new(&name);
        let job = PermanentJobObject::new(&name).unwrap();
        let mut child = long_running_process();
        job.assign_process(&child).unwrap();
        drop(job);

        let access = JOB_OBJECT_QUERY | JOB_OBJECT_SET_ATTRIBUTES | JOB_OBJECT_TERMINATE;
        let opened = PermanentJobObject::open(&name, access).unwrap().unwrap();
        opened.limit_active_processes(0).unwrap();
        opened.kill().unwrap();

        assert!(
            exits_within(&mut child, EXIT_TIMEOUT),
            "process kept running"
        );
    }

    #[rstest]
    #[case::assign_process(JOB_OBJECT_ASSIGN_PROCESS, true)]
    #[case::query(JOB_OBJECT_QUERY, false)]
    #[ignore = "requires root"]
    fn test_opened_job_accepts_processes_only_with_assign_process_as_root(
        #[case] access: u32,
        #[case] can_assign: bool,
    ) {
        let name = test_job_name(&format!("assign-{access:#x}"));
        let _cleanup = PermanentName::new(&name);
        let job = PermanentJobObject::new(&name).unwrap();
        let opened = PermanentJobObject::open(&name, access).unwrap().unwrap();
        let mut child = long_running_process();

        assert_eq!(opened.assign_process(&child).is_ok(), can_assign);

        let _ = child.kill();
        kill_and_wait(&job, vec![child]);
    }

    #[rstest]
    #[case::query(JOB_OBJECT_QUERY, true, false, false)]
    #[case::make_temporary(DELETE, false, true, false)]
    #[case::set_limits_without_query(JOB_OBJECT_SET_ATTRIBUTES, false, false, false)]
    #[case::query_and_set_limits(JOB_OBJECT_QUERY | JOB_OBJECT_SET_ATTRIBUTES, true, false, true)]
    #[ignore = "requires root"]
    fn test_opened_job_is_limited_to_the_requested_access_as_root(
        #[case] access: u32,
        #[case] can_query: bool,
        #[case] can_make_temporary: bool,
        #[case] can_limit: bool,
    ) {
        let name = test_job_name(&format!("access-{access:#x}"));
        let _cleanup = PermanentName::new(&name);
        let _job = PermanentJobObject::new(&name).unwrap();

        let opened = PermanentJobObject::open(&name, access).unwrap().unwrap();

        assert_eq!(opened.accounting().is_ok(), can_query);
        assert_eq!(opened.limit_active_processes(0).is_ok(), can_limit);
        assert_eq!(opened.make_temporary().is_ok(), can_make_temporary);
    }

    #[rstest]
    #[case::by_a_job("job", squat_with_a_job)]
    #[case::by_an_event("event", squat_with_an_event)]
    #[ignore = "requires root"]
    fn test_new_permanent_fails_if_the_name_is_taken_as_root(
        #[case] squatter: &str,
        #[case] squat: fn(&JobName) -> Box<dyn Any>,
    ) {
        let name = test_job_name(&format!("taken-by-{squatter}"));
        let _cleanup = PermanentName::new(&name);
        let _squatter = squat(&name);

        let err = PermanentJobObject::new(&name).err().unwrap();

        assert_eq!(
            err.to_string(),
            format!("creating permanent JobObject {name}: name already exists")
        );
    }

    #[test]
    #[ignore = "requires root"]
    fn test_new_permanent_sets_the_restrictive_security_descriptor_as_root() {
        let name = test_job_name("descriptor");
        let _cleanup = PermanentName::new(&name);

        let job = PermanentJobObject::new(&name).unwrap();

        let actual = owner_and_dacl_sddl(&job);
        let descriptor = SecurityDescriptor::for_current_user().unwrap();
        let expected = descriptor_sddl(PSECURITY_DESCRIPTOR(descriptor.as_ptr()));
        // Windows maps the generic `GA` right to the rights of a job, so only the owner, the
        // protection and who gets access are compared.
        assert!(actual.contains("D:P("), "{actual}");
        assert_eq!(owner_and_trustees(&actual), owner_and_trustees(&expected));
    }

    /// Enables `SeCreatePermanentPrivilege` and makes the permanent job `name` temporary when
    /// dropped, even if the test fails, so that the name does not outlive the test.
    struct PermanentName<'a>(&'a JobName);

    impl<'a> PermanentName<'a> {
        fn new(name: &'a JobName) -> Self {
            enable_privilege(SE_CREATE_PERMANENT_PRIVILEGE).expect(
                "run the test as an account holding SeCreatePermanentPrivilege, e.g. SYSTEM",
            );
            Self(name)
        }
    }

    impl Drop for PermanentName<'_> {
        fn drop(&mut self) {
            if let Ok(Some(job)) = PermanentJobObject::open(self.0, DELETE) {
                let _ = job.make_temporary();
            }
        }
    }

    fn squat_with_a_job(name: &JobName) -> Box<dyn Any> {
        Box::new(PermanentJobObject::new(name).unwrap())
    }

    fn squat_with_an_event(name: &JobName) -> Box<dyn Any> {
        let event = unsafe { CreateEventW(None, true, false, &HSTRING::from(name.as_ref())) };
        Box::new(unsafe { Owned::new(event.unwrap()) })
    }

    /// Job name unique to the test `tag` and this test process.
    fn test_job_name(tag: &str) -> JobName {
        JobName::for_executable("test", &format!("{tag}-{}", std::process::id())).unwrap()
    }

    /// Owner and DACL of the job, as SDDL.
    fn owner_and_dacl_sddl(job: &JobHandle) -> String {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let status = unsafe {
            GetSecurityInfo(
                *job.handle,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut descriptor),
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let _descriptor = LocalMemory(descriptor.0);
        descriptor_sddl(descriptor)
    }

    /// Owner and DACL of `descriptor`, as SDDL, with the aliases Windows uses (e.g. `SY`).
    fn descriptor_sddl(descriptor: PSECURITY_DESCRIPTOR) -> String {
        let mut sddl = PWSTR::null();
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut sddl,
                None,
            )
        }
        .unwrap();
        let _sddl = LocalMemory(sddl.0.cast());
        String::from_utf16_lossy(unsafe { sddl.as_wide() })
    }

    /// Owner and the account of each ACE of `sddl`, e.g. `("SY", ["SY", "SY"])`.
    fn owner_and_trustees(sddl: &str) -> (String, Vec<String>) {
        let (owner, dacl) = sddl.trim_start_matches("O:").split_once("D:").unwrap();
        let trustees = dacl
            .split(['(', ')'])
            .filter_map(|ace| ace.rsplit_once(';').map(|(_, trustee)| trustee.to_string()))
            .collect();
        (owner.to_string(), trustees)
    }
}
