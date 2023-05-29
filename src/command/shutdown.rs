#[cfg(target_family = "unix")]
use nix::{
    sys::signal,
    unistd::Pid,
};
use std::{
    sync::{
        Arc,
        Condvar,
        Mutex,
    },
    thread::sleep,
    time::Duration,
};

use super::{CommandError, CommandTerminator};

// In seconds
const DEFAULT_EXIT_TIMEOUT:u64 = 2;

pub struct ProcessTerminator{
    pid: u32,
    exit_timeout: u64,
}

impl ProcessTerminator {
    pub fn new(pid:u32) -> Self
    {
        Self {
            pid,
            exit_timeout: DEFAULT_EXIT_TIMEOUT
        }
    }

    pub fn with_custom_timeout(mut self, timeout:u64) -> Self
    {
        self.exit_timeout = timeout;
        return self;
    }
}

impl CommandTerminator for ProcessTerminator{
    type Error = CommandError;

    #[cfg(target_family = "unix")]
    fn shutdown(self, context: Arc<(Mutex<bool>, Condvar)>) -> Result<(), Self::Error> {
        let result_signal = signal::kill(Pid::from_raw(self.pid as i32), signal::SIGTERM);
        return match result_signal {
            Ok(signal) => {
                // Wait for the thread to start up.
                let (lock, cvar) = &*context;
                let mut exited = lock.lock().unwrap();

                loop {
                    let result = cvar.wait_timeout(exited, Duration::new(self.exit_timeout, 0)).unwrap();

                    exited = result.0;
                    let timer = result.1;
                    if timer.timed_out() {
                        _ = signal::kill(Pid::from_raw(self.pid as i32), signal::SIGKILL);
                        *exited = true;
                        cvar.notify_all();
                    }

                    if *exited == true {
                        break
                    }
                }

                Ok(signal)
            },
            Err(error) => Err(CommandError::from(error)),
        };
    }

    #[cfg(not(target_family = "unix"))]
    fn shutdown(self, context: Arc<(Mutex<bool>, Condvar)>) -> Result<(), Self::Error> {
        unimplemented!("windows processes can't be shutdown")
    }
}

#[cfg(target_family = "unix")]
#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        thread,
        time,
        sync::{
            Arc,
            Condvar,
            Mutex,
        },
    };
    use super::*;

    #[test]
    fn shutdown_default_timeout() {
        let mut trap_cmd = Command::new("sh")
            .arg("-c")
            .arg("trap \"sleep 35;exit 0\" TERM;while true; do sleep 1; done")
            .spawn();

        let pid = trap_cmd.as_mut().unwrap().id();
        let one_second = time::Duration::from_secs(1);
        sleep(one_second);

        let terminator =  ProcessTerminator::new(pid);

        let context = Arc::new((Mutex::new(false), Condvar::new()));
        let context_child = Arc::clone(&context);

        thread::spawn(|| {
            _ = terminator.shutdown(context_child);
        });

        // Wait for process to exit
        let result = trap_cmd.unwrap().wait();

        // We update the status o cvar to notify it exited
        let (lock, cvar) = &*context;
        let mut exited = lock.lock().unwrap();
        *exited = true;
        cvar.notify_all();

        assert_eq!("signal: 9 (SIGKILL)", result.unwrap().to_string());
    }

    #[test]
    fn shutdown_custom_timeout() {
        let mut trap_cmd = Command::new("sh")
            .arg("-c")
            .arg("trap \"sleep 35;exit 0\" TERM;while true; do sleep 1; done")
            .spawn();

        let pid = trap_cmd.as_mut().unwrap().id();
        let one_second = time::Duration::from_secs(1);
        sleep(one_second);

        let terminator =  ProcessTerminator::new(pid).with_custom_timeout(3);

        let context = Arc::new((Mutex::new(false), Condvar::new()));
        let context_child = Arc::clone(&context);

        thread::spawn(|| {
            _ = terminator.shutdown(context_child);
        });

        // Wait for process to exit
        let result = trap_cmd.unwrap().wait();

        // We update the status o cvar to notify it exited
        let (lock, cvar) = &*context;
        let mut exited = lock.lock().unwrap();
        *exited = true;
        cvar.notify_all();

        assert_eq!("signal: 9 (SIGKILL)", result.unwrap().to_string());
    }

    #[test]
    fn shutdown() {
        let mut trap_cmd = Command::new("sh")
            .arg("-c")
            .arg("trap \"sleep 1;exit 0\" TERM;while true; do sleep 1; done")
            .spawn();

        let pid = trap_cmd.as_mut().unwrap().id();
        let one_second = time::Duration::from_secs(1);
        sleep(one_second);

        let terminator =  ProcessTerminator::new(pid).with_custom_timeout(3);

        let context = Arc::new((Mutex::new(false), Condvar::new()));
        let context_child = Arc::clone(&context);

        thread::spawn(|| {
            _ = terminator.shutdown(context_child);
        });

        // Wait for process to exit
        let result = trap_cmd.unwrap().wait();

        // We update the status o cvar to notify it exited
        let (lock, cvar) = &*context;
        let mut exited = lock.lock().unwrap();
        *exited = true;
        cvar.notify_all();

        assert_eq!("exit status: 0", result.unwrap().to_string());
    }
}
