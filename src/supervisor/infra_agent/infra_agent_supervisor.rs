use std::marker::PhantomData;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::task::Context;
use std::thread;
use std::thread::JoinHandle;

use log::trace;
use nix::libc::pid_t;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;

use crate::command::{CommandExecutor, OutputStreamer, ProcessRunner};
use crate::ctx::{ContextDefault, Ctx};
use crate::stream::OutputEvent;
use crate::supervisor::supervisor::{Result, SupervisorExecutor, SupervisorHandle};

const BINARY_PATH: &str = "/usr/bin/newrelic-infra-service";

pub struct Unstarted;

pub struct Started;

pub struct InfraAgentSupervisorRunner<State = Unstarted> {
    std_sender: Option<Mutex<Sender<OutputEvent>>>,
    ctx: ContextDefault,
    pid: Arc<Mutex<u32>>,
    _marker:PhantomData<State>
}

impl SupervisorExecutor for InfraAgentSupervisorRunner {
    // type Error = CommandError;
    type Supervisor = InfraAgentSupervisorRunner<Started>;
    fn start(self) -> Result<Self::Supervisor> {
        trace!("starting infra agent supervisor");

        let sender = self.std_sender.unwrap().lock().unwrap().clone();
        let pid = Arc::clone(&self.pid);

        thread::spawn(move || {
            let mut exit_status: ExitStatus = ExitStatus::from_raw(0);
            while !self.ctx.is_cancelled() {
                match ProcessRunner::new(BINARY_PATH.to_string(), Vec::new()).start() {
                    Err(e) => eprintln!("started_process.is_err: {}", e),
                    Ok(started_process) => {
                        match started_process.stream(sender.clone()) {
                            Err(e) => eprintln!("cannot stream std {}", e),
                            Ok(mut streamed_process) => {
                                let mut l = pid.lock().unwrap();
                                *l = streamed_process.pid().clone();
                                match streamed_process.wait() {
                                    Err(e) => eprintln!("process died {}", e.to_string()),
                                    Ok(e) => {
                                        exit_status = e;
                                    }
                                }
                            }
                        }
                    }
                }

                //TODO apply restart policy to break infinite while
                // if started_process.is_err() {
                //     println!("started_process.is_err: {}", started_process.as_ref().err().unwrap().to_string());
                //     continue; // will apply restart policy
                // }
            }
            Ok(exit_status)
        });

        Ok(Self::Supervisor {
            std_sender: None,
            ctx: self.ctx,
            pid,
        })
    }
}


impl SupervisorHandle for InfraAgentSupervisorRunner<Started> {
    fn stop(mut self) -> Result<()> {
        self.ctx.cancel();
        signal::kill(Pid::from_raw(self.pid.lock().unwrap().clone() as pid_t), Signal::SIGTERM).unwrap();
        Ok(())
    }
}

impl InfraAgentSupervisorRunner {
    pub fn new(std_sender: Mutex<Sender<OutputEvent>>) -> Self {
        Self { ctx: ContextDefault::new(), std_sender: Some(std_sender), pid: Arc::new(Mutex::new(0)) }
    }
}



