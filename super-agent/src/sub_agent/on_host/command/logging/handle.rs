use std::{
    io::Read,
    process::{ChildStderr, ChildStdout},
};

use crate::sub_agent::logger::LogOutput;

pub(crate) enum LogHandle {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

// Make it usable as arg to `process_events`. Just delegating to the underlying ChildStdout/ChildStderr
impl Read for LogHandle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            LogHandle::Stdout(stdout) => stdout.read(buf),
            LogHandle::Stderr(stderr) => stderr.read(buf),
        }
    }
}

impl From<ChildStdout> for LogHandle {
    fn from(stdout: ChildStdout) -> Self {
        LogHandle::Stdout(stdout)
    }
}

impl From<ChildStderr> for LogHandle {
    fn from(stderr: ChildStderr) -> Self {
        LogHandle::Stderr(stderr)
    }
}

// A function pointer is also a primitive type
pub(super) type ToLogOutput = fn(String) -> LogOutput;
// And each "variant constructor" of an enum is also a function. In this implementation, given that
// all variants of `LogOutput` take a String to produce the LogOutput (so, fn(String) -> LogOutput),
// I can select what LogOutput variant to construct based on the LogHandle variant, without knowing
// the input string yet!
impl From<&LogHandle> for ToLogOutput {
    fn from(handle: &LogHandle) -> Self {
        match handle {
            LogHandle::Stdout(_) => LogOutput::Stdout,
            LogHandle::Stderr(_) => LogOutput::Stderr,
        }
    }
}
