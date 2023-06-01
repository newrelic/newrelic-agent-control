use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

// Use atomicbool instead?
// type SupervisorContext2 = Arc<(AtomicBool, Condvar)>;
// Or RwMutex?
// type SupervisorContext3 = Arc<(RwLock<bool>, Condvar)>;
pub struct SupervisorContext(Arc<(Mutex<bool>, Condvar)>);

impl SupervisorContext {
    pub(crate) fn new() -> Self {
        SupervisorContext(Arc::new((Mutex::new(false), Condvar::new())))
    }

    pub(crate) fn cancel_all(&self) -> Result<(), PoisonError<MutexGuard<'_, bool>>> /* this is the error type returned by a failed `lock()` */
    {
        let (lck, cvar) = &*self.0;
        *lck.lock()? = true;
        cvar.notify_all();
        Ok(())
    }

    pub(crate) fn get_lock_cvar(&self) -> &(Mutex<bool>, Condvar) {
        &*self.0
    }
}

impl Clone for SupervisorContext {
    fn clone(&self) -> Self {
        SupervisorContext(self.0.clone())
    }
}
