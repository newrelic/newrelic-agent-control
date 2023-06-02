use std::{
    marker::PhantomData,
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

pub struct Stopped;
pub struct Running;

#[derive(Debug)]
pub struct SupervisorRunner<State = Stopped> {
    bin: String,
    args: Vec<String>,
    ctx: SupervisorContext,
    snd: Sender<OutputEvent>,
    handle: Option<JoinHandle<()>>,
    state: PhantomData<State>,
}

impl Runner for SupervisorRunner<Stopped> {
    type E = ProcessError;
    type H = SupervisorRunner<Running>;

    fn run(self) -> Self::H {
        let handle = run_process_thread(
            self.bin.clone(),
            self.args.clone(),
            self.ctx.clone(),
            self.snd.clone(),
        );
        SupervisorRunner {
            bin: self.bin,
            args: self.args,
            ctx: self.ctx,
            snd: self.snd,
            handle: Some(handle),
            state: PhantomData,
        }
    }
}

fn run_process_thread(
    bin: String,
    args: Vec<String>,
    ctx: SupervisorContext,
    snd: Sender<OutputEvent>,
) -> JoinHandle<()> {
    thread::spawn({
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
            let streaming = match started.stream(snd.clone()) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to stream the output of a supervised process: {}", e);
                    continue;
                }
            };

            _ = wait_for_termination(streaming.get_pid(), ctx.clone());
            _ = streaming.wait().unwrap();

            let (lck, _) = SupervisorContext::get_lock_cvar(&ctx);
            let val = lck.lock().unwrap();
            if *val {
                break;
            }
        }
    })
}

/// Blocks on the [`SupervisorContext`], [`ctx`]. When the termination signal is activated, this will send a shutdown signal to the process being supervised (the one whose PID was passed as [`pid`]).
fn wait_for_termination(pid: u32, ctx: SupervisorContext) -> JoinHandle<()> {
    thread::spawn(move || {
        let (lck, cvar) = SupervisorContext::get_lock_cvar(&ctx);
        _ = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);

        thread::spawn(move || {
            let shutdown_ctx = Arc::new((Mutex::new(false), Condvar::new()));
            _ = ProcessTerminator::new(pid).shutdown(|| wait_exit_timeout_default(shutdown_ctx));
        });
    })
}

impl Handle for SupervisorRunner<Running> {
    type E = ProcessError;
    type S = JoinHandle<()>;

    fn get_handle(self) -> Option<Self::S> {
        self.handle
    }
}

impl SupervisorRunner<Stopped> {
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
            state: PhantomData,
            handle: None,
        }
    }
}
