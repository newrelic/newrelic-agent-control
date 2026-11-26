//! Console shared by every supervised process, used to deliver `CTRL_BREAK_EVENT` to them.
//!
//! Agent Control needs a console of its own so that supervised processes are spawned on it and
//! `GenerateConsoleCtrlEvent` reaches them directly. Interactive runs already have one; a Windows
//! service does not, so one is allocated on first use. The console is never swapped afterwards, so
//! spawning and signalling need no coordination.

use std::io;
use std::sync::Once;
use tracing::error;
use windows::Win32::System::Console::{
    AllocConsole, CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent, GetConsoleProcessList,
};

static CONSOLE_READY: Once = Once::new();

/// The console of the current process, shared by every supervised process. Get it through
/// [`Console::ensure`], which guarantees the console exists.
#[derive(Clone, Copy)]
pub(crate) struct Console;

impl Console {
    /// Makes sure the current process has a console, allocating one if it has none, and returns
    /// access to it. It is idempotent and must run before the first supervised process is
    /// spawned, so that it is spawned on this console. An allocation failure is logged once and
    /// never retried: signalling then fails and supervised processes are stopped forcefully.
    pub(crate) fn ensure() -> Self {
        CONSOLE_READY.call_once(|| {
            if !has_console() {
                alloc_console();
            }
        });
        Self
    }

    /// Sends `CTRL_BREAK_EVENT` to the process group `pid`, which must be the id of a process
    /// created with `CREATE_NEW_PROCESS_GROUP`. Only the processes of that group receive it.
    pub(super) fn send_ctrl_break(self, pid: u32) -> io::Result<()> {
        // SAFETY: plain Win32 call without pointers.
        unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) }.map_err(io::Error::from)
    }
}

fn has_console() -> bool {
    let mut process_list = [0u32; 1];
    // SAFETY: the buffer is valid for the length passed. The call returns 0 without a console.
    unsafe { GetConsoleProcessList(&mut process_list) != 0 }
}

fn alloc_console() {
    // SAFETY: plain Win32 call without pointers.
    if let Err(err) = unsafe { AllocConsole() } {
        error!("Could not allocate a console, sub-agents will be stopped forcefully: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_gives_the_process_a_console_and_is_idempotent() {
        Console::ensure();
        assert!(has_console());

        Console::ensure();
        assert!(has_console());
    }
}
