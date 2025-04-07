use crossbeam::channel::{unbounded, Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
/// A simple, unbounded broadcast channel for low-throughput use cases.
///
/// This struct allows multiple subscribers to receive broadcasted messages. Each subscriber
/// gets its own channel, ensuring that all subscribers receive all messages sent through
/// the broadcaster.
///
/// # Examples
///
/// ```rust
/// use newrelic_agent_control::event::broadcaster::unbounded::UnboundedBroadcast;
///
/// let mut broadcaster = UnboundedBroadcast::default();
///
/// let subscriber1 = broadcaster.subscribe();
/// let subscriber2 = broadcaster.subscribe();
///
/// broadcaster.broadcast("Hello, world!");
///
/// assert_eq!(subscriber1.recv().unwrap(), "Hello, world!");
/// assert_eq!(subscriber2.recv().unwrap(), "Hello, world!");
/// ```
///
/// # Notes
/// - This implementation is not optimized for high-throughput scenarios.
/// - Broadcasters aren't notified whenever a subscriber gets disconnected.
/// - Use with caution! these are unbounded channels!
pub struct UnboundedBroadcast<T> {
    subscribed_senders: Arc<Mutex<Vec<Sender<T>>>>,
}

impl<T> UnboundedBroadcast<T>
where
    T: Clone,
{
    /// Registers a new Receiver to the channel.
    pub fn subscribe(&mut self) -> Receiver<T> {
        let (tx, rx) = unbounded();

        self.subscribed_senders
            .lock()
            .expect("failed to acquire the lock")
            .push(tx);

        rx
    }

    /// Sends 'message' to all registered non-disconnected subscribers. This function doesn't block
    /// since the channel is unbounded. It doesn't fail either since Disconnected
    /// subscribers will be removed from the subscriber list.
    pub fn broadcast(&self, message: T) {
        self.subscribed_senders
            .lock()
            .expect("failed to acquire the lock")
            .retain(|s| s.send(message.clone()).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_sub() {
        let mut broadcaster = UnboundedBroadcast::default();

        let subs1 = broadcaster.subscribe();
        let subs2 = broadcaster.subscribe();

        let message = "message";
        broadcaster.broadcast(message);

        assert!(subs1.recv().unwrap().eq(message));
        assert!(subs2.recv().unwrap().eq(message));
    }
    #[test]
    fn test_multi_prod() {
        let mut broadcaster = UnboundedBroadcast::default();

        let cloned_broadcaster = broadcaster.clone();

        let subs1 = broadcaster.subscribe();

        let message1 = "foo";
        let message2 = "bar";
        broadcaster.broadcast(message1);
        cloned_broadcaster.broadcast(message2);

        assert!(subs1.recv().unwrap().eq(message1));
        assert!(subs1.recv().unwrap().eq(message2));
    }

    #[test]
    fn test_multi_prod_multi_subs() {
        let mut broadcaster = UnboundedBroadcast::default();

        let cloned_broadcaster = broadcaster.clone();

        let subs1 = broadcaster.subscribe();
        let subs2 = broadcaster.subscribe();

        let message1 = "foo";
        let message2 = "bar";
        broadcaster.broadcast(message1);
        cloned_broadcaster.broadcast(message2);

        assert!(subs1.recv().unwrap().eq(message1));
        assert!(subs1.recv().unwrap().eq(message2));

        assert!(subs2.recv().unwrap().eq(message1));
        assert!(subs2.recv().unwrap().eq(message2));
    }

    #[test]
    fn test_subscriber_drops() {
        let mut broadcaster = UnboundedBroadcast::default();

        let subs1 = broadcaster.subscribe();
        let subs2 = broadcaster.subscribe();

        drop(subs2);

        let message = "message";
        broadcaster.broadcast(message);

        assert!(subs1.recv().unwrap().eq(message));
    }

    #[test]
    fn test_broadcaster_drops() {
        let mut broadcaster = UnboundedBroadcast::default();

        let subs1 = broadcaster.subscribe();

        let message = "message";
        broadcaster.broadcast(message);
        drop(broadcaster);

        // receive queued message and fail because disconnect.
        assert!(subs1.recv().unwrap().eq(message));
        subs1.recv().unwrap_err();
    }
}
