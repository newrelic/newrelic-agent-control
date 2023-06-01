use std::{
    ffi::OsStr,
    sync::mpsc::Sender,
    thread::{self, JoinHandle},
};

use crate::command::{
    processrunner::Unstarted, stream::OutputEvent, CommandExecutor, CommandHandle, OutputStreamer,
    ProcessRunner,
};

use super::{context::SupervisorContext, error::ProcessError, Runner};

use log::error;

pub(crate) struct SupervisorRunner<
    Runner = ProcessRunner<Unstarted>,
    RunnerCollection = Vec<Runner>,
> where
    Runner: CommandExecutor,
    RunnerCollection: IntoIterator<Item = Runner>,
{
    runners: RunnerCollection,
}

impl Runner for SupervisorRunner {
    type E = ProcessError;

    fn run(
        self,
        ctx: SupervisorContext,
        tx: Sender<OutputEvent>,
    ) -> JoinHandle<Vec<Result<(), Self::E>>> {
        thread::spawn(move || {
            // Actually run the process
            let started = self
                .runners
                .into_iter()
                .map(|r| {
                    r.start().map_err(|e| {
                        error!("Failed to start a supervised process: {}", e);
                        ProcessError::ProcessNotStarted
                    })
                })
                .flatten() // FIXME: filter out the erroring processes
                .collect::<Vec<_>>();

            // TODO: stream output should be here?
            // Feel free to remove this!
            let streaming = started
                .into_iter()
                .map(|r| {
                    r.stream(tx.clone()).map_err(|e| {
                        error!("Failed to stream a supervised process: {}", e);
                        ProcessError::StreamError
                    })
                })
                .flatten()
                .collect::<Vec<_>>();

            // Wait for the signal that the process has finished to return
            let (lck, cvar) = SupervisorContext::get_lock_cvar(&ctx);
            let _guard = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

            // Stop all the processes
            streaming
                .into_iter()
                .map(|r| {
                    r.stop().map_err(|e| {
                        error!("Failed to stop a supervised process: {}", e);
                        ProcessError::StopProcessError
                    })
                })
                .collect::<Vec<_>>()
        })
    }
}

impl SupervisorRunner<ProcessRunner<Unstarted>, Option<ProcessRunner<Unstarted>>> {
    fn new<S, I>(bin: S, args: I) -> Self
    where
        S: AsRef<OsStr>,
        I: IntoIterator<Item = S>,
    {
        SupervisorRunner {
            runners: Some(ProcessRunner::new(bin, args)),
        }
    }
}

impl SupervisorRunner {
    pub fn new<S, I>(bin: S, args: I) -> Self
    where
        S: AsRef<OsStr>,
        I: IntoIterator<Item = S>,
    {
        SupervisorRunner {
            runners: vec![ProcessRunner::new(bin, args)],
        }
    }

    // FIXME: feel free to remove this!
    pub fn with_restart_policy(self) -> Self {
        unimplemented!("todo");
    }
}
