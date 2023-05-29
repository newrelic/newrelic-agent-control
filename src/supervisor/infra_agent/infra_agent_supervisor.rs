use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use std::thread;
use std::thread::JoinHandle;

use log::trace;
use nix::libc::pid_t;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;

use crate::command::{CommandExecutor, OutputStreamer, ProcessRunner};
use crate::ctx::Ctx;
use crate::stream::OutputEvent;
use crate::supervisor::supervisor::{Result, Supervisor};

pub struct InfraAgentSupervisor<C> where C: Ctx + Send + Sync {
    std_sender: Option<Mutex<Sender<OutputEvent>>>,
    ctx: C,
    pid: Arc<Mutex<u32>>,
}

const BINARY_PATH: &str = "/usr/bin/newrelic-infra-service";

#[allow(unused_variables)] // TODO temporary until decide what to do with handle
impl<C> Supervisor for InfraAgentSupervisor<C> where C: Ctx + Send + Sync + Clone + 'static {
    fn start(&mut self) -> Result<()> {
        trace!("starting infra agent supervisor");

        let supervisor_ctx = self.ctx.clone();
        let sender = self.std_sender.take().unwrap().lock().unwrap().clone();

        let pid = Arc::clone(&self.pid);

        let handle: JoinHandle<Result<ExitStatus>> = thread::spawn(move || {
            let mut exit_status: ExitStatus = ExitStatus::from_raw(0);
            while !supervisor_ctx.is_cancelled() {
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

        Ok(())
    }

    fn stop(&mut self) -> crate::supervisor::supervisor::Result<()> {
        self.ctx.cancel();
        signal::kill(Pid::from_raw(self.pid.lock().unwrap().clone() as pid_t), Signal::SIGTERM).unwrap();
        Ok(())
    }
}

impl<C> InfraAgentSupervisor<C> where C: Ctx + Send + Sync {
    pub fn new(ctx: C, std_sender: Mutex<Sender<OutputEvent>>) -> Self {
        Self { ctx, std_sender: Some(std_sender), pid: Arc::new(Mutex::new(0)) }
    }
}



