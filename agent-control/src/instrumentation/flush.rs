//! Bounds OTel SDK flush calls. `force_flush()` has no built-in timeout of its own - its only
//! bound is the OTLP HTTP client's `client_timeout` (30s by default), which fires deep inside the
//! call. That's too long to block shutdown or a self-update on a hung/unreachable OTLP endpoint.

use opentelemetry_sdk::error::OTelSdkResult;
use std::sync::mpsc;
use std::time::Duration;

/// Default bound for a single flush attempt. Well under the OTLP client's own 30s timeout, since
/// a flush is expected to be fast under normal conditions - this exists specifically to cut the
/// wait short when the endpoint is unreachable or hung.
pub const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs `flush_fn` (typically a provider's `force_flush()`) on a separate thread and waits at
/// most `timeout` for it to finish. If it doesn't complete in time, logs a warning and returns
/// immediately - the flush thread keeps running in the background (still bounded by the OTLP
/// client's own request timeout), but the caller is never blocked waiting for it.
pub fn force_flush_with_timeout<F>(what: &'static str, timeout: Duration, flush_fn: F)
where
    F: FnOnce() -> OTelSdkResult + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Ignore send errors: the receiver may have already timed out and moved on.
        let _ = tx.send(flush_fn());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(error = %e, what, "failed to flush OTLP telemetry"),
        Err(_) => tracing::warn!(
            what,
            timeout_secs = timeout.as_secs(),
            "OTLP flush timed out, continuing without waiting for it"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    #[test]
    fn completes_promptly_when_flush_is_fast() {
        let start = std::time::Instant::now();
        force_flush_with_timeout("test", Duration::from_secs(5), || Ok(()));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn returns_promptly_when_flush_hangs() {
        let start = std::time::Instant::now();
        force_flush_with_timeout("test", Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(2));
            Ok(())
        });
        // Caller must not wait for the full 2s hang - only the 50ms timeout.
        assert!(start.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn surfaces_flush_errors_without_panicking() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();
        force_flush_with_timeout("test", Duration::from_secs(5), move || {
            called_clone.store(true, Ordering::SeqCst);
            Err(opentelemetry_sdk::error::OTelSdkError::InternalFailure(
                "simulated failure".to_string(),
            ))
        });
        assert!(called.load(Ordering::SeqCst));
    }
}
