use super::super::spawned_process::{error::CommandError, os_process::OsProcess};

/// Owns a Windows process HANDLE and closes it on drop, keeping the process object alive long
/// enough to reliably retrieve the exit code even for very short-lived processes.
pub(crate) struct ProcessHandle(windows::Win32::Foundation::HANDLE);

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
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .map(Self)
                .map_err(std::io::Error::from)
                .map_err(CommandError::from)
        }
    }

    /// Returns the current exit code, or `None` if the handle is no longer accessible.
    /// `Some(259)` means the process is still running (STILL_ACTIVE).
    pub(crate) fn exit_code(&self) -> Option<u32> {
        use windows::Win32::System::Threading::GetExitCodeProcess;
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.0, &mut code).ok().map(|_| code) }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
