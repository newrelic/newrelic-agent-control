use crate::supervisor::context::SupervisorContext;
#[cfg(target_family = "unix")]
use libc::{SIGINT, SIGTERM};
use log::info;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::thread::{self, sleep, JoinHandle};
use std::time::Duration;

static mut SHUT_DOWN: AtomicBool = AtomicBool::new(false);

pub struct SignalManager {
    ctx: SupervisorContext,
}

impl SignalManager {
    pub fn new(ctx: SupervisorContext) -> Self {
        SignalManager { ctx }
    }

    #[cfg(target_family = "unix")]
    pub fn shutdown_handle(self) -> JoinHandle<()> {
        register_shutdown_signal_handlers();

        thread::spawn({
            move || loop {
                unsafe {
                    if SHUT_DOWN.load(Relaxed) {
                        info!("Gracefully Shutting down");
                        self.ctx.cancel_all().unwrap();
                        break;
                    }
                }

                sleep(Duration::from_millis(500));
            }
        })
    }

    #[cfg(not(target_family = "unix"))]
    pub fn shutdown_handle(self) -> JoinHandle<()> {
        unimplemented!("windows processes can't be shutdown")
    }
}

#[cfg(target_family = "unix")]
fn register_shutdown_signal_handlers() {
    unsafe {
        libc::signal(SIGTERM, handle_shutdown_signal as usize);
        libc::signal(SIGINT, handle_shutdown_signal as usize);
    }
}

#[allow(dead_code)]
#[cfg(target_family = "unix")]
fn handle_shutdown_signal(_: i32) {
    // Reregister signals as soon as possible to minimize
    // signal changes affecting the signal handler itself.
    register_shutdown_signal_handlers();

    unsafe {
        SHUT_DOWN.store(true, Relaxed);
    }
}

#[cfg(target_family = "unix")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_sig_term_end_context() {
        let ctx = SupervisorContext::new();
        let manager = SignalManager::new(ctx.clone());

        manager.shutdown_handle();

        unsafe {
            libc::raise(SIGTERM);
        }

        let (lck, cvar) = SupervisorContext::get_lock_cvar(&ctx);
        _ = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);
    }

    #[test]
    fn catch_sig_int_end_context() {
        let ctx = SupervisorContext::new();
        let manager = SignalManager::new(ctx.clone());

        manager.shutdown_handle();

        unsafe {
            libc::raise(SIGINT);
        }

        let (lck, cvar) = SupervisorContext::get_lock_cvar(&ctx);
        _ = cvar.wait_while(lck.lock().unwrap(), |finish| !*finish);
    }
}
