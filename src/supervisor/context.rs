use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

#[derive(Debug, Clone, Default)]
pub struct SupervisorContext(Arc<(Mutex<bool>, Condvar)>);

impl SupervisorContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the cancellation signal. All threads that are waiting for this signal (i.e. were passed this [`SupervisorContext`] are notified so they unblock and finish execution, cancelling the processes.
    pub fn cancel_all(&self) -> Result<(), PoisonError<MutexGuard<'_, bool>>> /* this is the error type returned by a failed `lock()` */
    {
        let (lck, cvar) = &*self.0;
        *lck.lock()? = true;
        cvar.notify_all();
        Ok(())
    }

    pub(crate) fn get_lock_cvar(&self) -> &(Mutex<bool>, Condvar) {
        &self.0
    }
}
