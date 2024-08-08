use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

#[derive(Debug, Clone, Default)]
pub struct Context<T>(Arc<(Mutex<T>, Condvar)>);

impl<T> Context<T>
where
    T: Default,
{
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the cancellation signal. All threads that are waiting for this signal (i.e. were passed this [`Context`] are notified so they unblock and finish execution, cancelling the processes.
    pub fn cancel_all(&self, val: T) -> Result<(), PoisonError<MutexGuard<'_, T>>> /* this is the error type returned by a failed `lock()` */
    {
        let (lck, cvar) = &*self.0;
        let mut lck = lck.lock()?;
        *lck = val;
        cvar.notify_all();
        Ok(())
    }

    pub fn get_lock_cvar(&self) -> &(Mutex<T>, Condvar) {
        &self.0
    }

    /// Resets the Mutex to the default T value
    pub fn reset(&self) -> Result<(), PoisonError<MutexGuard<'_, T>>> /* this is the error type returned by a failed `lock()` */
    {
        let (lck, _) = &*self.0;
        let mut lck = lck.lock()?;
        *lck = <T as Default>::default();
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::context::Context;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::thread;
    use std::time::Duration;

    // Test that waiting on the condvar doesn't block the context from being read by another thread.
    #[test]
    fn test_context_can_be_cancelled_while_waiting() {
        let after_cancel = Arc::new(Mutex::new(()));
        let after_cancel_clone = after_cancel.clone();
        let ctx: Context<bool> = Context::new();
        let ctx_clone = ctx.clone();
        let guard = after_cancel.lock().unwrap();
        thread::spawn(move || {
            // wait for the wait_while to be started.
            thread::sleep(Duration::from_secs(1));

            // read the context value while other thread is waiting for the condvar
            assert!(!*Context::get_lock_cvar(&ctx_clone).0.lock().unwrap());

            // cancel the context
            ctx_clone.cancel_all(true).unwrap();

            println!("lock the context from the thread,there was no deadlock");

            let _guard = after_cancel_clone.lock().unwrap();

            // context still usable and unlocked after cancel
            assert!(*Context::get_lock_cvar(&ctx_clone).0.lock().unwrap());
        });

        let (lck, cvar) = Context::get_lock_cvar(&ctx);
        drop(cvar.wait_while(lck.lock().unwrap(), |finish| !*finish));

        drop(guard);

        println!("there was no deadlock");

        let _guard = after_cancel.lock().unwrap();
    }
}
