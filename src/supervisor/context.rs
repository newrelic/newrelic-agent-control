use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

pub struct SupervisorContext(Arc<(Mutex<bool>, Condvar)>);

impl SupervisorContext {
    pub fn new() -> Self {
        Self::default()
    }

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

impl Clone for SupervisorContext {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Default for SupervisorContext {
    fn default() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }
}
