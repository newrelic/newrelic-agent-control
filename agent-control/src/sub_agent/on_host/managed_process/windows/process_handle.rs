use super::super::spawned_process::{error::CommandError, os_process::OsProcess};
use tracing::warn;
use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

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

    /// Returns the exit code once the process has exited, or `None` if it is still running.
    ///
    /// Returns `Some(1)` if the handle is no longer accessible — this should not happen since
    /// `ProcessHandle` keeps the kernel object alive until dropped, but is safer than panicking.
    pub(crate) fn exit_code(&self) -> Option<u32> {
        let mut code = 0u32;
        match unsafe { GetExitCodeProcess(self.0, &mut code) } {
            Ok(_) if code == STILL_ACTIVE.0 as u32 => None,
            Ok(_) => Some(code),
            Err(e) => {
                // ERROR_INVALID_HANDLE: the retained handle became invalid unexpectedly.
                warn!(error = %e, "GetExitCodeProcess failed on retained handle; reporting exit code 1");
                Some(1)
            }
        }
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
