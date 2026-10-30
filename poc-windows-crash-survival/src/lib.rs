//! Minimal PoC of crash survival for sub-agents on Windows without elevation.
//!
//! Agent Control assigns each sub-agent to an anonymous Job Object created WITHOUT
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so the kernel closing the job handle on a crash does
//! not kill the sub-agent. A restarted instance re-adopts the process from bookkept
//! `(pid, start_time)` values. A named job cannot be used for re-adoption: its name is released
//! when the last handle closes (see the `job_name_probe` example).
#![cfg(windows)]

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::time::Duration;
use windows::Wdk::Foundation::{
    NtQueryObject, OBJECT_ATTRIBUTES, OBJECT_INFORMATION_CLASS, OBJECT_NAME_INFORMATION,
};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE, LUID,
    OBJ_CASE_INSENSITIVE, OBJ_PERMANENT, OBJECT_ATTRIBUTE_FLAGS, UNICODE_STRING, WAIT_OBJECT_0,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
    SE_PRIVILEGE_ENABLED, SECURITY_DESCRIPTOR, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION,
    TOKEN_PRIVILEGES, TOKEN_PRIVILEGES_ATTRIBUTES, TOKEN_QUERY, TokenElevation,
};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList, OpenJobObjectW,
    QueryInformationJobObject,
};
use windows::Win32::System::SystemServices::JOB_OBJECT_QUERY;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_BASIC_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, PROCESS_VM_READ, TerminateProcess, WaitForSingleObject,
};
use windows::core::{HRESULT, HSTRING, PCWSTR, PWSTR};

/// `STILL_ACTIVE` exit code reported by `GetExitCodeProcess` for a running process.
const STILL_ACTIVE: u32 = 259;

#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct PocError(String);

fn err(context: &str, e: impl std::fmt::Display) -> PocError {
    PocError(format!("{context}: {e}"))
}

/// Owned Win32 handle closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // Safety: the handle is owned by this wrapper and closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Snapshot of a job's membership and accounting counters.
#[derive(Debug)]
pub struct JobInfo {
    pub active_processes: u32,
    pub total_processes: u32,
    pub pids: Vec<usize>,
    /// Object manager path of the job (e.g. `\Sessions\1\BaseNamedObjects\Local\x`). `None`
    /// for anonymous jobs and when querying without a handle.
    pub name: Option<String>,
}

/// Mirrors `JOBOBJECT_BASIC_PROCESS_ID_LIST` with room for 64 pids.
#[repr(C)]
struct PidList {
    assigned: u32,
    in_list: u32,
    pids: [usize; 64],
}

/// `ObjectNameInformation`, not exposed as a named constant by the `windows` crate.
const OBJECT_NAME_INFORMATION_CLASS: OBJECT_INFORMATION_CLASS = OBJECT_INFORMATION_CLASS(1);

/// Reads the object manager name of `handle`, if it has one.
fn object_name(handle: HANDLE) -> Result<Option<String>, PocError> {
    // `u64` elements keep the buffer aligned for `OBJECT_NAME_INFORMATION`.
    let mut buf = [0u64; 256];
    let status = unsafe {
        NtQueryObject(
            Some(handle),
            OBJECT_NAME_INFORMATION_CLASS,
            Some(buf.as_mut_ptr() as *mut _),
            size_of_val(&buf) as u32,
            None,
        )
    };
    status.ok().map_err(|e| err("querying job name", e))?;
    // Safety: on success the buffer starts with an `OBJECT_NAME_INFORMATION` whose string
    // points inside the same buffer.
    let name = unsafe {
        let info = &*(buf.as_ptr() as *const OBJECT_NAME_INFORMATION);
        let chars = std::slice::from_raw_parts(info.Name.Buffer.0, info.Name.Length as usize / 2);
        String::from_utf16_lossy(chars)
    };
    Ok((!name.is_empty()).then_some(name))
}

/// Queries `job`; a `None` handle means the job of the calling process.
fn query_job(job: Option<HANDLE>) -> Result<JobInfo, PocError> {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            &mut accounting as *mut _ as *mut _,
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            None,
        )
    }
    .map_err(|e| err("querying job accounting", e))?;

    let mut list = PidList {
        assigned: 0,
        in_list: 0,
        pids: [0; 64],
    };
    unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicProcessIdList,
            &mut list as *mut _ as *mut _,
            size_of::<PidList>() as u32,
            None,
        )
    }
    .map_err(|e| err("querying job process list", e))?;

    let name = job.map(object_name).transpose()?.flatten();

    Ok(JobInfo {
        name,
        active_processes: accounting.ActiveProcesses,
        total_processes: accounting.TotalProcesses,
        pids: list.pids[..list.in_list as usize].to_vec(),
    })
}

/// Job object without kill-on-close: dropping it only closes the handle.
pub struct Job(Handle);

impl Job {
    /// Creates an anonymous job. No limits are set on purpose.
    pub fn new() -> Result<Self, PocError> {
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|e| err("creating job", e))?;
        Ok(Self(Handle(handle)))
    }

    /// Creates a named job, or opens it if it already exists. Used by `job_name_probe`.
    pub fn create(name: &str) -> Result<Self, PocError> {
        let handle = unsafe { CreateJobObjectW(None, &HSTRING::from(name)) }
            .map_err(|e| err("creating named job", e))?;
        Ok(Self(Handle(handle)))
    }

    /// Opens an existing named job. Used by `job_name_probe`.
    pub fn open(name: &str) -> Result<Self, PocError> {
        let handle = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, false, &HSTRING::from(name)) }
            .map_err(|e| err("opening named job", e))?;
        Ok(Self(Handle(handle)))
    }

    /// Queries the job through this handle.
    pub fn info(&self) -> Result<JobInfo, PocError> {
        query_job(Some(self.0.0))
    }

    /// Queries the job the calling process belongs to, which needs no handle or name.
    pub fn current_info() -> Result<JobInfo, PocError> {
        query_job(None)
    }

    pub fn assign(&self, child: &Child) -> Result<(), PocError> {
        let process = HANDLE(child.as_raw_handle());
        unsafe { AssignProcessToJobObject(self.0.0, process) }
            .map_err(|e| err("assigning process to job", e))
    }
}

/// Wait-able handle to any process (child or not), the Windows analogue of an adopted PID.
pub struct AdoptedProcess(Handle);

impl AdoptedProcess {
    fn try_open(pid: u32) -> windows::core::Result<Self> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                false,
                pid,
            )
        }?;
        Ok(Self(Handle(handle)))
    }

    pub fn open(pid: u32) -> Result<Self, PocError> {
        Self::try_open(pid).map_err(|e| err(&format!("opening process {pid}"), e))
    }

    /// Adopts `pid` only if it is alive and its start time matches the bookkept one, which
    /// guards against pid reuse.
    pub fn adopt(pid: u32, recorded_start_time: u64) -> Result<Option<Self>, PocError> {
        let process = match Self::try_open(pid) {
            Ok(process) => process,
            // `OpenProcess` reports a pid that does not exist as invalid parameter.
            Err(e) if e.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) => {
                return Ok(None);
            }
            Err(e) => return Err(err(&format!("opening process {pid}"), e)),
        };
        let adoptable = process.is_alive()? && process.start_time()? == recorded_start_time;
        Ok(adoptable.then_some(process))
    }

    /// Terminates the process. Descendants are not affected.
    pub fn kill(&self) -> Result<(), PocError> {
        unsafe { TerminateProcess(self.0.0, 1) }.map_err(|e| err("terminating process", e))
    }

    /// Creation time as a `FILETIME` value, used as anti-PID-reuse marker.
    pub fn start_time(&self) -> Result<u64, PocError> {
        let (mut creation, mut exit, mut kernel, mut user) = Default::default();
        unsafe { GetProcessTimes(self.0.0, &mut creation, &mut exit, &mut kernel, &mut user) }
            .map_err(|e| err("reading process times", e))?;
        Ok(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
    }

    /// An exited process stays openable while any handle to it exists, so check the exit code.
    pub fn is_alive(&self) -> Result<bool, PocError> {
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.0.0, &mut code) }
            .map_err(|e| err("reading exit code", e))?;
        Ok(code == STILL_ACTIVE)
    }

    /// Returns `true` if the process exited within `timeout`.
    pub fn wait(&self, timeout: Duration) -> bool {
        unsafe { WaitForSingleObject(self.0.0, timeout.as_millis() as u32) == WAIT_OBJECT_0 }
    }
}

/// `ProcessCommandLineInformation`, not exposed as a named constant by the `windows` crate.
const PROCESS_COMMAND_LINE_CLASS: PROCESSINFOCLASS = PROCESSINFOCLASS(60);

// Offsets inside the 64-bit `PEB` and `RTL_USER_PROCESS_PARAMETERS`, which the `windows` crate
// only exposes as opaque reserved fields.
#[cfg(target_pointer_width = "64")]
const PEB_PROCESS_PARAMETERS: usize = 0x20;
#[cfg(target_pointer_width = "64")]
const PARAMS_ENVIRONMENT: usize = 0x80;
#[cfg(target_pointer_width = "64")]
const PARAMS_ENVIRONMENT_SIZE: usize = 0x3F0;

/// Upper bound for the environment block copied out of another process.
const MAX_ENV_BYTES: usize = 256 * 1024;

fn open_for_inspection(pid: u32) -> Result<Handle, PocError> {
    unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
    }
    .map(Handle)
    .map_err(|e| err(&format!("opening process {pid} for inspection"), e))
}

/// Reads a `T` from the address space of `process`.
///
/// Safety: `T` must be valid for any bit pattern.
unsafe fn read_remote<T: Default>(process: &Handle, addr: usize) -> Result<T, PocError> {
    let mut out = T::default();
    unsafe {
        ReadProcessMemory(
            process.0,
            addr as *const _,
            &mut out as *mut T as *mut _,
            size_of::<T>(),
            None,
        )
    }
    .map_err(|e| err("reading remote memory", e))?;
    Ok(out)
}

/// Command line of any same-user process, readable after the job and its name are gone.
pub fn process_command_line(pid: u32) -> Result<String, PocError> {
    let process = open_for_inspection(pid)?;
    // `u64` elements keep the buffer aligned for `UNICODE_STRING`.
    let mut buf = [0u64; 1024];
    let mut returned = 0u32;
    unsafe {
        NtQueryInformationProcess(
            process.0,
            PROCESS_COMMAND_LINE_CLASS,
            buf.as_mut_ptr() as *mut _,
            size_of_val(&buf) as u32,
            &mut returned,
        )
    }
    .ok()
    .map_err(|e| err("querying process command line", e))?;
    // Safety: on success the buffer starts with a `UNICODE_STRING` pointing inside the buffer.
    let line = unsafe {
        let s = &*(buf.as_ptr() as *const windows::Win32::Foundation::UNICODE_STRING);
        String::from_utf16_lossy(std::slice::from_raw_parts(
            s.Buffer.0,
            s.Length as usize / 2,
        ))
    };
    Ok(line)
}

/// Value of the environment variable `key` as seen by process `pid` at start-up.
pub fn process_env_var(pid: u32, key: &str) -> Result<Option<String>, PocError> {
    let process = open_for_inspection(pid)?;
    let mut pbi = PROCESS_BASIC_INFORMATION::default();
    let mut returned = 0u32;
    unsafe {
        NtQueryInformationProcess(
            process.0,
            PROCESSINFOCLASS(0), // ProcessBasicInformation
            &mut pbi as *mut _ as *mut _,
            size_of::<PROCESS_BASIC_INFORMATION>() as u32,
            &mut returned,
        )
    }
    .ok()
    .map_err(|e| err("querying process basic information", e))?;

    // Safety: all reads below are plain integers.
    let (env_ptr, env_size) = unsafe {
        let params: usize = read_remote(
            &process,
            pbi.PebBaseAddress as usize + PEB_PROCESS_PARAMETERS,
        )?;
        let env_ptr: usize = read_remote(&process, params + PARAMS_ENVIRONMENT)?;
        let env_size: usize = read_remote(&process, params + PARAMS_ENVIRONMENT_SIZE)?;
        (env_ptr, env_size.min(MAX_ENV_BYTES))
    };

    let mut block = vec![0u16; env_size / 2];
    unsafe {
        ReadProcessMemory(
            process.0,
            env_ptr as *const _,
            block.as_mut_ptr() as *mut _,
            env_size,
            None,
        )
    }
    .map_err(|e| err("reading environment block", e))?;

    let prefix = format!("{}=", key.to_uppercase());
    Ok(block
        .split(|c| *c == 0)
        .take_while(|entry| !entry.is_empty())
        .map(String::from_utf16_lossy)
        .find(|entry| entry.to_uppercase().starts_with(&prefix))
        .map(|entry| entry[prefix.len()..].to_string()))
}

/// Result of scanning process environments for a tag.
#[derive(Debug, Default)]
pub struct EnvTagScan {
    /// Pids whose environment has the requested `key=value`.
    pub matches: Vec<u32>,
    /// Processes whose environment could be read.
    pub inspected: usize,
    /// Processes that could not be opened or read (other users, protected ones).
    pub skipped: usize,
    /// Processes discarded by the image name filter without being opened.
    pub filtered_out: usize,
}

/// Scans other processes for an environment containing `key=value`. When `image_names` is set,
/// only processes whose executable name matches one of them (case-insensitive) are opened.
pub fn find_processes_by_env_tag(
    key: &str,
    value: &str,
    image_names: Option<&[&str]>,
) -> Result<EnvTagScan, PocError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(Handle)
        .map_err(|e| err("creating process snapshot", e))?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut scan = EnvTagScan::default();
    let mut next = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while next.is_ok() {
        let pid = entry.th32ProcessID;
        if pid != std::process::id() {
            if let Some(names) = image_names {
                let image = String::from_utf16_lossy(
                    &entry.szExeFile[..entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)],
                );
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&image)) {
                    scan.filtered_out += 1;
                    next = unsafe { Process32NextW(snapshot.0, &mut entry) };
                    continue;
                }
            }
            match process_env_var(pid, key) {
                Ok(found) => {
                    scan.inspected += 1;
                    if found.as_deref() == Some(value) {
                        scan.matches.push(pid);
                    }
                }
                Err(_) => scan.skipped += 1,
            }
        }
        next = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(scan)
}

/// Pids of the processes whose recorded parent is `parent_pid`, read from the process snapshot
/// only (no process is opened). Windows keeps the parent pid after the parent exits.
pub fn find_processes_by_parent(parent_pid: u32) -> Result<Vec<u32>, PocError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(Handle)
        .map_err(|e| err("creating process snapshot", e))?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut pids = Vec::new();
    let mut next = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while next.is_ok() {
        if entry.th32ParentProcessID == parent_pid {
            pids.push(entry.th32ProcessID);
        }
        next = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(pids)
}

/// Whether the current process token is elevated (admin). The PoC is meant to run with `false`.
pub fn is_elevated() -> Result<bool, PocError> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|e| err("opening process token", e))?;
    let token = Handle(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }
    .map_err(|e| err("reading token elevation", e))?;
    Ok(elevation.TokenIsElevated != 0)
}

/// Native entry points that have no Win32 equivalent or are not exposed by the `windows` crate.
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
        pub fn NtOpenJobObject(
            job_handle: *mut HANDLE,
            desired_access: u32,
            object_attributes: *const OBJECT_ATTRIBUTES,
        ) -> i32;
        pub fn NtOpenDirectoryObject(
            directory_handle: *mut HANDLE,
            desired_access: u32,
            object_attributes: *const OBJECT_ATTRIBUTES,
        ) -> i32;
        pub fn NtMakeTemporaryObject(handle: HANDLE) -> i32;
        pub fn NtQueryDirectoryObject(
            directory_handle: HANDLE,
            buffer: *mut core::ffi::c_void,
            length: u32,
            return_single_entry: u8,
            restart_scan: u8,
            context: *mut u32,
            return_length: *mut u32,
        ) -> i32;
    }
}

const DELETE: u32 = 0x0001_0000;
const DIRECTORY_QUERY: u32 = 0x1;
const JOB_OBJECT_ALL_ACCESS: u32 = 0x1F_001F;
const STATUS_MORE_ENTRIES: i32 = 0x0000_0105;
const STATUS_NO_MORE_ENTRIES: i32 = 0x8000_001Au32 as i32;
/// `ObjectBasicInformation`.
const OBJECT_BASIC_INFORMATION_CLASS: OBJECT_INFORMATION_CLASS = OBJECT_INFORMATION_CLASS(0);

fn status_err(context: &str, status: i32) -> PocError {
    PocError(format!("{context}: NTSTATUS {:#010X}", status as u32))
}

/// Builds `OBJECT_ATTRIBUTES` for the object manager path `path` and passes it to `call`, which
/// returns an `NTSTATUS`.
fn with_attributes(
    path: &str,
    attributes: u32,
    call: impl FnOnce(*const OBJECT_ATTRIBUTES) -> i32,
) -> i32 {
    with_security(path, attributes, std::ptr::null(), call)
}

/// Same as [`with_attributes`] with an explicit security descriptor (null for the default one).
fn with_security(
    path: &str,
    attributes: u32,
    security_descriptor: *const SECURITY_DESCRIPTOR,
    call: impl FnOnce(*const OBJECT_ATTRIBUTES) -> i32,
) -> i32 {
    let mut wide: Vec<u16> = path.encode_utf16().collect();
    let len = (wide.len() * 2) as u16;
    let name = UNICODE_STRING {
        Length: len,
        MaximumLength: len,
        Buffer: PWSTR(wide.as_mut_ptr()),
    };
    let attrs = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        ObjectName: &name,
        Attributes: OBJECT_ATTRIBUTE_FLAGS(OBJ_CASE_INSENSITIVE.0 | attributes),
        SecurityDescriptor: security_descriptor,
        ..Default::default()
    };
    call(&attrs)
}

fn open_native_job(nt_path: &str, access: u32) -> Result<Handle, PocError> {
    let mut handle = HANDLE::default();
    let status = with_attributes(nt_path, 0, |attrs| unsafe {
        nt::NtOpenJobObject(&mut handle, access, attrs)
    });
    if status < 0 {
        return Err(status_err(&format!("opening job {nt_path}"), status));
    }
    Ok(Handle(handle))
}

impl Job {
    /// Creates a named job at the object manager path `nt_path` (e.g.
    /// `\BaseNamedObjects\name`). A `permanent` job keeps its name after the last handle is
    /// closed and requires `SeCreatePermanentPrivilege` to be enabled in the token.
    pub fn create_native(nt_path: &str, permanent: bool) -> Result<Self, PocError> {
        let mut handle = HANDLE::default();
        let attributes = if permanent { OBJ_PERMANENT.0 } else { 0 };
        let status = with_attributes(nt_path, attributes, |attrs| unsafe {
            nt::NtCreateJobObject(&mut handle, JOB_OBJECT_ALL_ACCESS, attrs)
        });
        if status < 0 {
            return Err(status_err(&format!("creating job {nt_path}"), status));
        }
        Ok(Self(Handle(handle)))
    }
}

/// Turns a permanent job back into a temporary one, so its name goes away with the last handle.
pub fn make_temporary(nt_path: &str) -> Result<(), PocError> {
    let job = open_native_job(nt_path, DELETE | JOB_OBJECT_QUERY)?;
    let status = unsafe { nt::NtMakeTemporaryObject(job.0) };
    if status < 0 {
        return Err(status_err("making job temporary", status));
    }
    Ok(())
}

/// A job found by name in an object manager directory.
#[derive(Debug)]
pub struct NamedJob {
    /// Object manager path, e.g. `\BaseNamedObjects\nr-ac-demo`.
    pub path: String,
    pub name: String,
    /// `OBJ_PERMANENT` is set: the name outlives its handles until `make_temporary`.
    pub permanent: bool,
    /// Handles to the job held by processes, not counting the one used to inspect it.
    pub other_handles: u32,
    pub active_processes: u32,
    pub pids: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct JobListing {
    pub jobs: Vec<NamedJob>,
    /// Directory entries of other types (events, mutexes, sections...).
    pub other_types: usize,
    /// Jobs that could not be opened for query (access denied).
    pub unreadable: usize,
}

/// `OBJECT_DIRECTORY_INFORMATION`.
#[repr(C)]
struct ObjectDirectoryInformation {
    name: UNICODE_STRING,
    type_name: UNICODE_STRING,
}

/// `OBJECT_BASIC_INFORMATION`.
#[repr(C)]
#[derive(Default)]
struct ObjectBasicInformation {
    attributes: u32,
    granted_access: u32,
    handle_count: u32,
    pointer_count: u32,
    paged_pool_charge: u32,
    non_paged_pool_charge: u32,
    reserved: [u32; 3],
    name_info_size: u32,
    type_info_size: u32,
    security_descriptor_size: u32,
    creation_time: i64,
}

fn unicode_string(s: &UNICODE_STRING) -> String {
    if s.Buffer.is_null() {
        return String::new();
    }
    // Safety: the string points inside the buffer filled by `NtQueryDirectoryObject`.
    String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(s.Buffer.0, s.Length as usize / 2)
    })
}

fn inspect_job(path: String, name: String) -> Result<NamedJob, PocError> {
    let job = open_native_job(&path, JOB_OBJECT_QUERY)?;
    let mut basic = ObjectBasicInformation::default();
    unsafe {
        NtQueryObject(
            Some(job.0),
            OBJECT_BASIC_INFORMATION_CLASS,
            Some(&mut basic as *mut _ as *mut _),
            size_of::<ObjectBasicInformation>() as u32,
            None,
        )
    }
    .ok()
    .map_err(|e| err("querying job attributes", e))?;
    let info = query_job(Some(job.0))?;
    Ok(NamedJob {
        path,
        name,
        permanent: basic.attributes & OBJ_PERMANENT.0 != 0,
        // The handle used here is counted by the kernel.
        other_handles: basic.handle_count.saturating_sub(1),
        active_processes: info.active_processes,
        pids: info.pids,
    })
}

/// Lists the named jobs in the object manager `directory` (e.g. `\BaseNamedObjects` or
/// `\Sessions\1\BaseNamedObjects`). Only named objects are listed, and a temporary job only
/// appears while some process holds a handle to it.
pub fn list_named_jobs(directory: &str) -> Result<JobListing, PocError> {
    let mut dir = HANDLE::default();
    let status = with_attributes(directory, 0, |attrs| unsafe {
        nt::NtOpenDirectoryObject(&mut dir, DIRECTORY_QUERY, attrs)
    });
    if status < 0 {
        return Err(status_err(
            &format!("opening directory {directory}"),
            status,
        ));
    }
    let dir = Handle(dir);

    // `u64` elements keep the buffer aligned for `ObjectDirectoryInformation`.
    let mut buf = vec![0u64; 8192];
    let mut context = 0u32;
    let mut restart = 1u8;
    let mut listing = JobListing::default();
    loop {
        let mut returned = 0u32;
        let status = unsafe {
            nt::NtQueryDirectoryObject(
                dir.0,
                buf.as_mut_ptr() as *mut _,
                (buf.len() * size_of::<u64>()) as u32,
                0,
                restart,
                &mut context,
                &mut returned,
            )
        };
        restart = 0;
        if status == STATUS_NO_MORE_ENTRIES {
            break;
        }
        if status < 0 {
            return Err(status_err(
                &format!("listing directory {directory}"),
                status,
            ));
        }

        // The entries end with a zeroed one.
        let mut entry = buf.as_ptr() as *const ObjectDirectoryInformation;
        loop {
            // Safety: the kernel wrote a zero-terminated array of entries into `buf`.
            let current = unsafe { &*entry };
            if current.name.Buffer.is_null() {
                break;
            }
            if unicode_string(&current.type_name).eq_ignore_ascii_case("Job") {
                let name = unicode_string(&current.name);
                let path = format!("{directory}\\{name}");
                match inspect_job(path, name) {
                    Ok(job) => listing.jobs.push(job),
                    Err(_) => listing.unreadable += 1,
                }
            } else {
                listing.other_types += 1;
            }
            entry = unsafe { entry.add(1) };
        }
        if status != STATUS_MORE_ENTRIES {
            break;
        }
    }
    Ok(listing)
}

/// Enables or disables the privilege `name` (e.g. `SeCreatePermanentPrivilege`) in the process
/// token. Returns `false` when the token does not hold it, which is the case for accounts that
/// were not granted the user right.
pub fn set_privilege(name: &str, enabled: bool) -> Result<bool, PocError> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    }
    .map_err(|e| err("opening process token", e))?;
    let token = Handle(token);

    let mut luid = LUID::default();
    unsafe { LookupPrivilegeValueW(PCWSTR::null(), &HSTRING::from(name), &mut luid) }
        .map_err(|e| err(&format!("looking up privilege {name}"), e))?;
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: if enabled {
                SE_PRIVILEGE_ENABLED
            } else {
                TOKEN_PRIVILEGES_ATTRIBUTES(0)
            },
        }],
    };
    unsafe { AdjustTokenPrivileges(token.0, false, Some(&privileges), 0, None, None) }
        .map_err(|e| err(&format!("adjusting privilege {name}"), e))?;
    // The call succeeds even if the token lacks the privilege and reports it through last error.
    Ok(unsafe { GetLastError() } != ERROR_NOT_ALL_ASSIGNED)
}

/// Security descriptor inspection and creation for named jobs.
mod security {
    use super::*;
    use std::ffi::c_void;
    use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
        ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
    };
    use windows::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACCESS_DENIED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION,
        AclSizeInformation, DACL_SECURITY_INFORMATION, GetAce, GetAclInformation,
        LookupAccountSidW, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE,
        TOKEN_USER, TokenUser,
    };

    const SDDL_REVISION_1: u32 = 1;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;

    /// Memory allocated by the system that must be released with `LocalFree`.
    struct LocalMem(*mut c_void);

    impl Drop for LocalMem {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // Safety: the pointer was allocated by the system with `LocalAlloc`.
                unsafe { LocalFree(Some(HLOCAL(self.0))) };
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum AceKind {
        Allow,
        Deny,
        /// Audit, label, object ACEs... not expected in a job DACL.
        Other(u8),
    }

    /// One entry of a DACL.
    #[derive(Debug, Clone)]
    pub struct Ace {
        pub kind: AceKind,
        pub sid: String,
        /// `DOMAIN\name` when the SID can be resolved.
        pub account: Option<String>,
        pub mask: u32,
    }

    /// Owner and DACL of a kernel object.
    #[derive(Debug)]
    pub struct ObjectSecurity {
        pub owner: String,
        pub owner_account: Option<String>,
        pub sddl: String,
        /// `None` is a NULL DACL: everybody has every access.
        pub aces: Option<Vec<Ace>>,
    }

    impl ObjectSecurity {
        /// The owner when it is not in `allowed_owners`. Only the creator (or a holder of
        /// `SeTakeOwnershipPrivilege`) can set the owner, so a job created by another account under
        /// the same name is reported here.
        pub fn unexpected_owner(&self, allowed_owners: &[&str]) -> Option<String> {
            (!allowed_owners.contains(&self.owner.as_str())).then(|| {
                self.owner_account
                    .clone()
                    .unwrap_or_else(|| self.owner.clone())
            })
        }

        /// Allow ACEs whose trustee is not in `allowed_sids`, plus every NULL DACL.
        /// An empty result means the DACL grants nothing to anybody else.
        pub fn unexpected_grants(&self, allowed_sids: &[&str]) -> Vec<String> {
            match &self.aces {
                None => vec!["NULL DACL (everyone, full access)".to_string()],
                Some(aces) => aces
                    .iter()
                    .filter(|a| a.kind == AceKind::Allow && !allowed_sids.contains(&a.sid.as_str()))
                    .map(|a| {
                        format!(
                            "{} mask {:#010X}",
                            a.account.as_deref().unwrap_or(&a.sid),
                            a.mask
                        )
                    })
                    .collect(),
            }
        }
    }

    fn sid_to_string(sid: PSID) -> Result<String, PocError> {
        let mut out = PWSTR::null();
        unsafe { ConvertSidToStringSidW(sid, &mut out) }.map_err(|e| err("converting SID", e))?;
        let _guard = LocalMem(out.0 as *mut c_void);
        unsafe { out.to_string() }.map_err(|e| err("decoding SID", e))
    }

    fn account_name(sid: PSID) -> Option<String> {
        let (mut name_len, mut domain_len) = (0u32, 0u32);
        let mut kind = SID_NAME_USE::default();
        // The first call only reports the required buffer sizes.
        let _ = unsafe {
            LookupAccountSidW(
                PCWSTR::null(),
                sid,
                None,
                &mut name_len,
                None,
                &mut domain_len,
                &mut kind,
            )
        };
        if name_len == 0 {
            return None;
        }
        let mut name = vec![0u16; name_len as usize];
        let mut domain = vec![0u16; domain_len.max(1) as usize];
        unsafe {
            LookupAccountSidW(
                PCWSTR::null(),
                sid,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                Some(PWSTR(domain.as_mut_ptr())),
                &mut domain_len,
                &mut kind,
            )
        }
        .ok()?;
        let name = String::from_utf16_lossy(&name[..name_len as usize]);
        let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
        Some(if domain.is_empty() {
            name
        } else {
            format!("{domain}\\{name}")
        })
    }

    fn read_aces(acl: *const ACL) -> Result<Vec<Ace>, PocError> {
        let mut info = ACL_SIZE_INFORMATION::default();
        unsafe {
            GetAclInformation(
                acl,
                &mut info as *mut _ as *mut c_void,
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        }
        .map_err(|e| err("reading ACL size", e))?;

        (0..info.AceCount)
            .map(|index| {
                let mut raw = std::ptr::null_mut::<c_void>();
                unsafe { GetAce(acl, index, &mut raw) }.map_err(|e| err("reading ACE", e))?;
                // Allowed and denied ACEs share the layout: header, mask, SID.
                let header = unsafe { *(raw as *const ACE_HEADER) };
                let (mask, sid) = match header.AceType {
                    ACCESS_ALLOWED_ACE_TYPE => {
                        let ace = raw as *const ACCESS_ALLOWED_ACE;
                        (unsafe { (*ace).Mask }, unsafe {
                            &raw const (*ace).SidStart
                        })
                    }
                    ACCESS_DENIED_ACE_TYPE => {
                        let ace = raw as *const ACCESS_DENIED_ACE;
                        (unsafe { (*ace).Mask }, unsafe {
                            &raw const (*ace).SidStart
                        })
                    }
                    other => {
                        return Ok(Ace {
                            kind: AceKind::Other(other),
                            sid: String::new(),
                            account: None,
                            mask: 0,
                        });
                    }
                };
                let sid = PSID(sid as *mut c_void);
                Ok(Ace {
                    kind: if header.AceType == ACCESS_ALLOWED_ACE_TYPE {
                        AceKind::Allow
                    } else {
                        AceKind::Deny
                    },
                    sid: sid_to_string(sid)?,
                    account: account_name(sid),
                    mask,
                })
            })
            .collect()
    }

    /// Reads owner and DACL of `handle`, which needs `READ_CONTROL`.
    pub fn read(handle: HANDLE) -> Result<ObjectSecurity, PocError> {
        let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let (mut owner, mut dacl) = (PSID::default(), std::ptr::null_mut::<ACL>());
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                info,
                Some(&mut owner),
                None,
                Some(&mut dacl),
                None,
                Some(&mut descriptor),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(err("GetSecurityInfo", format!("error {}", status.0)));
        }
        // `owner` and `dacl` point inside `descriptor`.
        let _guard = LocalMem(descriptor.0);

        let mut sddl = PWSTR::null();
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                info,
                &mut sddl,
                None,
            )
        }
        .map_err(|e| err("converting descriptor to SDDL", e))?;
        let sddl_guard = LocalMem(sddl.0 as *mut c_void);
        let sddl = unsafe { sddl.to_string() }.map_err(|e| err("decoding SDDL", e))?;
        drop(sddl_guard);

        Ok(ObjectSecurity {
            owner: sid_to_string(owner)?,
            owner_account: account_name(owner),
            sddl,
            aces: if dacl.is_null() {
                None
            } else {
                Some(read_aces(dacl)?)
            },
        })
    }

    /// SID string of the user in the current process token.
    pub fn current_user_sid() -> Result<String, PocError> {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|e| err("opening process token", e))?;
        let token = Handle(token);
        // `u64` elements keep the buffer aligned for `TOKEN_USER`.
        let mut buffer = vec![0u64; 64];
        let mut returned = 0u32;
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(buffer.as_mut_ptr() as *mut c_void),
                (buffer.len() * size_of::<u64>()) as u32,
                &mut returned,
            )
        }
        .map_err(|e| err("reading token user", e))?;
        let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
        sid_to_string(user.User.Sid)
    }

    /// SDDL that grants full access only to `SYSTEM` and to the user `sid`, and blocks
    /// inheritance (`P`). Everybody else, administrators included, gets no ACE.
    pub fn restrictive_sddl(user_sid: &str) -> String {
        const SYSTEM_SID: &str = "S-1-5-18";
        if user_sid == SYSTEM_SID {
            "D:P(A;;GA;;;SY)".to_string()
        } else {
            format!("D:P(A;;GA;;;SY)(A;;GA;;;{user_sid})")
        }
    }

    /// Parses `sddl` into a self-relative descriptor and passes it to `call`.
    pub fn with_sddl<T>(
        sddl: &str,
        call: impl FnOnce(*const SECURITY_DESCRIPTOR) -> Result<T, PocError>,
    ) -> Result<T, PocError> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|e| err(&format!("parsing SDDL {sddl}"), e))?;
        let _guard = LocalMem(descriptor.0);
        call(descriptor.0 as *const SECURITY_DESCRIPTOR)
    }
}

pub use security::{Ace, AceKind, ObjectSecurity, current_user_sid, restrictive_sddl};

impl Job {
    /// Same as [`Job::create_native`] with an explicit DACL given as SDDL, applied atomically at
    /// creation so there is no window with the default DACL.
    pub fn create_native_with_sddl(
        nt_path: &str,
        permanent: bool,
        sddl: &str,
    ) -> Result<Self, PocError> {
        security::with_sddl(sddl, |descriptor| {
            let mut handle = HANDLE::default();
            let attributes = if permanent { OBJ_PERMANENT.0 } else { 0 };
            let status = with_security(nt_path, attributes, descriptor, |attrs| unsafe {
                nt::NtCreateJobObject(&mut handle, JOB_OBJECT_ALL_ACCESS, attrs)
            });
            if status < 0 {
                return Err(status_err(&format!("creating job {nt_path}"), status));
            }
            Ok(Self(Handle(handle)))
        })
    }

    /// Opens an existing named job through the native API with the requested access mask.
    pub fn open_native(nt_path: &str, access: u32) -> Result<Self, PocError> {
        open_native_job(nt_path, access).map(Self)
    }

    /// Owner and DACL of the job. The handle needs `READ_CONTROL`, which a job created by this
    /// process always has.
    pub fn security(&self) -> Result<ObjectSecurity, PocError> {
        security::read(self.0.0)
    }
}

/// `READ_CONTROL` standard access right: read the security descriptor.
pub const READ_CONTROL: u32 = 0x0002_0000;

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn long_running_child() -> Child {
        Command::new("ping")
            .args(["-n", "300", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawning ping")
    }

    /// In-process version of the crash scenario: closing every handle to the job must not kill
    /// the process, and it must be re-adoptable from the bookkept values.
    #[test]
    fn child_survives_job_handle_close_and_can_be_adopted() {
        let mut child = long_running_child();
        let pid = child.id();
        let start_time = AdoptedProcess::open(pid).unwrap().start_time().unwrap();

        let job = Job::new().unwrap();
        job.assign(&child).unwrap();
        drop(job);

        let adopted = AdoptedProcess::adopt(pid, start_time).unwrap().unwrap();
        assert!(!adopted.wait(Duration::from_millis(200)));

        adopted.kill().unwrap();
        assert!(adopted.wait(Duration::from_secs(5)));
        assert!(!adopted.is_alive().unwrap());
        let _ = child.wait();
    }

    #[test]
    fn mismatched_start_time_is_not_adopted() {
        let mut child = long_running_child();
        let start_time = AdoptedProcess::open(child.id())
            .unwrap()
            .start_time()
            .unwrap();

        assert!(
            AdoptedProcess::adopt(child.id(), start_time + 1)
                .unwrap()
                .is_none()
        );

        child.kill().unwrap();
        let _ = child.wait();
    }

    #[test]
    fn named_job_is_listed_with_its_attributes() {
        let name = format!("nr-ac-test-{}", std::process::id());
        let path = format!(r"\BaseNamedObjects\{name}");
        let _job = Job::create_native(&path, false).unwrap();

        let listing = list_named_jobs(r"\BaseNamedObjects").unwrap();
        let found = listing.jobs.iter().find(|j| j.name == name).unwrap();
        assert!(!found.permanent);
        assert_eq!(found.other_handles, 1);
        assert_eq!(found.path, path);
    }

    /// Needs no privilege: a temporary job gets the same DACL treatment as a permanent one.
    #[test]
    fn restrictive_sddl_leaves_only_user_and_system() {
        let user = current_user_sid().unwrap();
        let name = format!("nr-ac-dacl-test-{}", std::process::id());
        let path = format!(r"\BaseNamedObjects\{name}");

        let job = Job::create_native_with_sddl(&path, false, &restrictive_sddl(&user)).unwrap();
        let security = job.security().unwrap();

        assert_eq!(security.unexpected_owner(&[&user, "S-1-5-32-544"]), None);
        assert!(security.unexpected_owner(&["S-1-1-0"]).is_some());
        assert!(security.unexpected_grants(&[&user, "S-1-5-18"]).is_empty());
        assert!(security.aces.as_ref().unwrap().len() <= 2);
    }
}
