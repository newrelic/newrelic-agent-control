use std::sync::{Arc, Condvar, Mutex};

pub trait Ctx where {
    fn cancel(&mut self);
    fn wait(&self);
    fn is_cancelled(&self) -> bool;
}

pub struct ContextDefault {
    cancelled: Arc<(Mutex<bool>, Condvar)>,
    parent: Option<Arc<ContextDefault>>,
}

impl Clone for ContextDefault {
    fn clone(&self) -> Self {
        let parent = match &self.parent {
            None => None,
            Some(x) => Some(x.clone())
        };
        Self { cancelled: Arc::clone(&self.cancelled), parent }
    }
}

impl ContextDefault {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new((Mutex::new(false), Condvar::new())),
            parent: None,
        }
    }
}

impl Ctx for ContextDefault {
    fn cancel(&mut self) {
        let (cancelled, cvar) = &*self.cancelled;
        let mut cancelled = cancelled.lock().unwrap();
        *cancelled = true;
        // We notify the condvar that the value has changed.
        cvar.notify_all();
    }

    fn wait(&self) {
        // Wait for the thread to start up.
        let (lock, cvar) = &*self.cancelled;
        let mut cancelled = lock.lock().unwrap();
        while !*cancelled {
            cancelled = cvar.wait(cancelled).unwrap();
        }
    }

    fn is_cancelled(&self) -> bool {
        let (cancelled, _) = &*self.cancelled;
        *cancelled.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time};
    use std::thread::JoinHandle;

    use ntest::timeout;

    use super::*;

    #[test]
    #[timeout(10)]
    #[should_panic]
    fn not_cancelled_context_waits_forever() {
        let ctx = ContextDefault::new();
        // just created context is not cancelled
        assert_eq!(false, ctx.is_cancelled());

        // next call will be blocking
        ctx.wait()
    }


    #[test]
    fn context_can_be_cancelled() {
        let mut ctx = ContextDefault::new();
        // just created context is not cancelled
        assert_eq!(false, ctx.is_cancelled());

        // cancelled context is cancelled
        ctx.cancel();
        assert_eq!(true, ctx.is_cancelled());

        // next call will not be blocking
        ctx.wait()
    }

    #[test]
    fn cancelled_context_can_be_cancelled() {
        let mut ctx = ContextDefault::new();
        assert_eq!(false, ctx.is_cancelled());

        ctx.cancel();
        assert_eq!(true, ctx.is_cancelled());

        // cancelling a cancelled context should be possible
        ctx.cancel();
        assert_eq!(true, ctx.is_cancelled());
    }

    #[test]
    fn context_shared_between_threads() {
        // store messages to assert the order of execution of separated threads
        let messages: Arc<Mutex<Vec<&str>>> = Arc::new(Mutex::new(Vec::new()));

        let ctx = ContextDefault::new();

        // clone context and messages to be used in a separated thread
        let ctx_copy = ctx.clone();
        let messages_ref = messages.clone();

        // store thread handles
        let mut handles: Vec<JoinHandle<()>> = Vec::new();

        // thread waiting to context cancellation
        handles.push(
            thread::spawn(move || {
                assert_eq!(false, ctx_copy.is_cancelled());
                ctx_copy.wait();
                assert_eq!(true, ctx_copy.is_cancelled());
                messages_ref.lock().unwrap().push("first thread context cancelled");
            })
        );

        // clone context and messages to be used in a separated thread
        let mut ctx_copy = ctx.clone();
        let messages_ref = messages.clone();

        // cancel context form a separate thread
        handles.push(
            thread::spawn(move || {
                thread::sleep(time::Duration::from_millis(50));
                messages_ref.lock().unwrap().push("second_thread before cancel");

                assert_eq!(false, ctx_copy.is_cancelled());
                ctx_copy.cancel();

                assert_eq!(true, ctx_copy.is_cancelled());
                thread::sleep(time::Duration::from_millis(10));
                messages_ref.lock().unwrap().push("second_thread after cancelled");
            })
        );

        messages.lock().unwrap().push("main_thread waiting");
        assert_eq!(false, ctx.is_cancelled());
        ctx.wait();
        assert_eq!(true, ctx.is_cancelled());

        handles.into_iter().for_each(move |h| { h.join().unwrap(); });

        let expected_messages = vec![
            "main_thread waiting",
            "second_thread before cancel",
            "first thread context cancelled",
            "second_thread after cancelled",
        ];

        assert_eq!(expected_messages, messages.lock().unwrap().to_vec());
    }

    #[test]
    fn context_shared_between_multiple_threads() {
        let counts: Arc<Mutex<u8>> = Arc::new(Mutex::new(0));

        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let mut ctx = ContextDefault::new();

        (0..5).for_each(|_| {
            let ctx_copy = ctx.clone();
            let counts_ref = counts.clone();
            handles.push(
                thread::spawn(move || {
                    assert_eq!(false, ctx_copy.is_cancelled());
                    ctx_copy.wait();
                    assert_eq!(true, ctx_copy.is_cancelled());

                    // shared struct to track executions
                    let mut c = counts_ref.lock().unwrap();
                    *c += 1;
                })
            );
        });

        assert_eq!(0, *counts.lock().unwrap());
        assert_eq!(false, ctx.is_cancelled());
        // cancel the context after a few millis to  give time to threads to spawn
        thread::sleep(time::Duration::from_millis(20));
        ctx.cancel();

        //wait until all the threads
        handles.into_iter().for_each(move |h| { h.join().unwrap(); });
        //assert execution count
        assert_eq!(5, *counts.lock().unwrap());
    }
}