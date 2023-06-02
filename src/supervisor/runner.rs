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

pub(crate) struct SupervisorRunner {
    // runner: Runner,
    bin: String,
    args: Vec<String>,
    context: SupervisorContext,
    sender: Sender<OutputEvent>,
}

impl Runner for SupervisorRunner {
    type E = ProcessError;

    fn run(&mut self) -> JoinHandle<Result<(), Self::E>> {
        thread::spawn({
            let ctx = self.context.clone();
            let tx = self.sender.clone();
            let bin = self.bin.clone();
            let args = self.args.clone();
            move || loop {
                let runner = ProcessRunner::new(&bin, &args);
                // Actually run the process
                let started = match runner.start() {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to start a supervised process: {}", e);
                        continue;
                    }
                };

                // Stream the output
                let streaming = match started.stream(tx.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to stream the output of a supervised process: {}", e);
                        continue;
                    }
                };

                let pid = streaming.get_pid();

                let ctx_c = ctx.clone();

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
                let (lck, _) = SupervisorContext::get_lock_cvar(&ctx);

                let val = lck.lock().unwrap();
                if *val == true {
                    break Ok(());
                }

                // // Wait for the signal that the process has finished to return
                // let (lck, cvar) = SupervisorContext::get_lock_cvar(&ctx);
                // let _guard = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

                // // Stop all the processes
                // streaming
                //     .into_iter()
                //     .map(|r| {
                //         r.stop().map_err(|e| {
                //             error!("Failed to stop a supervised process: {}", e);
                //             ProcessError::StopProcessError
                //         })
                //     })
                //     .collect::<Vec<_>>()
            }
        })
    }
}

impl Handle for SupervisorRunner {
    type E = ProcessError;

    fn stop(self) -> Result<(), Self::E> {
        self.context.cancel_all().unwrap();
        Ok(())
    }
}

impl SupervisorRunner {
    // pub fn new<S, I>(bin: S, args: I) -> Self
    // where
    // S: AsRef<OsStr>,
    // I: IntoIterator<Item = S>,
    pub fn new(
        bin: String,
        args: Vec<String>,
        ctx: SupervisorContext,
        snd: Sender<OutputEvent>,
    ) -> Self {
        SupervisorRunner {
            bin: bin,
            args: args,
            context: ctx,
            sender: snd,
        }
    }

    // FIXME: feel free to remove this!
    pub fn with_restart_policy(self) -> Self {
        unimplemented!("todo");
    }
}
