use std::{
    sync::mpsc::Sender,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};

use crate::command::{
    stream::OutputEvent, wait_exit_timeout_default, CommandExecutor, CommandHandle,
    CommandTerminator, OutputStreamer, ProcessRunner, ProcessTerminator,
};

use super::{context::SupervisorContext, error::ProcessError, Handle, Runner};

use log::error;

type ProcessHandle = JoinHandle<Result<(), ProcessError>>;

pub struct SupervisorRunner {
    bin: String,
    args: Vec<String>,
    ctx: SupervisorContext,
    snd: Sender<OutputEvent>,
}

pub struct SupervisorHandle(ProcessHandle);

impl Runner for SupervisorRunner {
    type E = ProcessError;
    type H = SupervisorHandle;

    fn run(self) -> Self::H {
        SupervisorHandle(thread::spawn({
            move || loop {
                let runner = ProcessRunner::new(&self.bin, &self.args);

                // Actually run the process
                let started = match runner.start() {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to start a supervised process: {}", e);
                        continue;
                    }
                };

                // Stream the output
                let streaming = match started.stream(self.snd.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to stream the output of a supervised process: {}", e);
                        continue;
                    }
                };

                let pid = streaming.get_pid();

                let ctx_c = self.ctx.clone();

                let _thread_handle = thread::spawn(move || {
                    let (lck, cvar) = SupervisorContext::get_lock_cvar(&ctx_c);
                    let _guard = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

                    thread::spawn(move || {
                        let shutdown_ctx = Arc::new((Mutex::new(false), Condvar::new()));
                        let terminator = ProcessTerminator::new(pid);
                        _ = terminator.shutdown(|| wait_exit_timeout_default(shutdown_ctx));
                    });
                });

                let _waiting = streaming.wait();

                //Check this
                let (lck, _) = SupervisorContext::get_lock_cvar(&self.ctx);

                let val = lck.lock().unwrap();
                if *val {
                    break Ok(());
                }
            }
        }))
    }
}

impl Handle for SupervisorHandle {
    type E = ProcessError;

    fn get_handles(self) -> ProcessHandle {
        self.0
    }
}

impl SupervisorRunner {
    pub fn new(
        bin: String,
        args: Vec<String>,
        ctx: SupervisorContext,
        snd: Sender<OutputEvent>,
    ) -> Self {
        SupervisorRunner {
            bin,
            args,
            ctx,
            snd,
        }
    }
}
