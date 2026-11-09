use super::super::spawned_process::{error::CommandError, os_process::OsProcess};
use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE as STILL_ACTIVE_NTSTATUS};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

pub(super) const STILL_ACTIVE: u32 = STILL_ACTIVE_NTSTATUS.0 as u32;

/// Owns a Windows process HANDLE and closes it on drop, keeping the process object alive long
/// enough to reliably retrieve the exit code even for very short-lived processes.
pub(crate) struct ProcessHandle(HANDLE);

impl ProcessHandle {
    /// Opens a secondary handle to `os_process`, guaranteeing it refers to the same process
    /// object. Safe because `os_process` holds the `Child` handle that keeps the kernel object
    /// alive, so the PID cannot be recycled before this call returns.
    pub(crate) fn from_os_process(os_process: &OsProcess) -> Result<Self, CommandError> {
        Self::open(os_process.get_pid())
    }

    /// Opens a handle to an already-running process by PID alone. The caller must ensure the
    /// PID has not been recycled since it was observed (no `Child` handle to pin it).
    pub(crate) fn open(pid: u32) -> Result<Self, CommandError> {
        unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .map(Self)
                .map_err(std::io::Error::from)
                .map_err(CommandError::from)
        }
    }

    /// Returns the current exit code, or `None` if the handle is no longer accessible.
    /// `Some(STILL_ACTIVE)` means the process is still running.
    pub(crate) fn exit_code(&self) -> Option<u32> {
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.0, &mut code).ok().map(|_| code) }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // Closing the handle releases our reference to the kernel process object. Once all
        // handles to the object are closed (including the OS-managed one held by the process
        // itself after exit), Windows frees the object and its exit code becomes unavailable.
        // This is safe to call after the process exits because the object persists as long as
        // at least one handle remains open.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
