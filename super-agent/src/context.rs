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
