use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossbeam::{
    channel::{bounded, tick, Receiver, SendError, Sender},
    select,
};

static CHAN_SIZE: usize = 1;

/// The error enum for Ticker errors.
#[derive(Debug, thiserror::Error)]
pub enum TickerError {
    /// Error variant indicating that the ticker is cancelled.
    #[error("ticker cancelled")]
    Cancelled,
    /// Error variant for SendError with associated TickerEvent.
    #[error("`{0}`")]
    SendError(#[from] SendError<TickerEvent>),
}

/// The Ticker trait defining the functions `next`, `reset`, and `stop`.
pub trait Ticker {
    /// Returns the unit value if a tick was fired. Returns an error if the channel is closed.
    fn next(&self) -> Result<(), TickerError>;

    /// Reset the ticker. Returns an error if unable to reset.
    fn reset(&self) -> Result<(), TickerError>;

    /// Stop the ticker. Returns an error if unable to stop.
    fn stop(&self) -> Result<(), TickerError>;
}

/// The events that control the behavior of the ticker.
pub enum TickerEvent {
    /// Event to reset the ticker.
    Reset,

    /// Event to stop the ticker.
    Stop,
}

pub struct CrossBeamTicker {
    /// The duration between ticks.
    duration: Duration,

    /// The receiver for receiving reset and stop events.
    reset_receiver: Arc<Mutex<Receiver<TickerEvent>>>,

    /// The sender for sending reset and stop events.
    reset_sender: Sender<TickerEvent>,
}

impl CrossBeamTicker {
    pub(super) fn new(next_timeout: Duration) -> Self {
        let (reset_sender, reset_receiver) = bounded(CHAN_SIZE);
        Self {
            duration: next_timeout,
            reset_receiver: Arc::new(Mutex::new(reset_receiver)),
            reset_sender,
        }
    }
}

impl Ticker for CrossBeamTicker {
    fn next(&self) -> Result<(), TickerError> {
        let tick = tick(self.duration);
        // TODO: remove unwrap
        let reset_receiver = self.reset_receiver.lock().unwrap();

        select! {
            recv(tick) -> _ => Ok(()),
            recv(reset_receiver) -> reset_result => match reset_result {
                Ok(event) => match event {
                    TickerEvent::Reset => {
                        drop(reset_receiver);
                        self.next()
                    },
                    TickerEvent::Stop => Err(TickerError::Cancelled),
                },
                Err(_) => Err(TickerError::Cancelled),
            },
        }
    }

    fn stop(&self) -> Result<(), TickerError> {
        Ok(self.reset_sender.send(TickerEvent::Stop)?)
    }

    fn reset(&self) -> Result<(), TickerError> {
        Ok(self.reset_sender.send(TickerEvent::Reset)?)
    }
}

#[cfg(test)]
pub(super) mod test {
    use super::*;
    use mockall::mock;

    // Ticker mock for testing purposes.
    mock! {
        pub(crate) TickerMockAll {}

        impl Ticker for TickerMockAll {
            fn next(&self) -> Result<(), TickerError>;
            fn reset(&self) -> Result<(), TickerError>;
            fn stop(&self) -> Result<(), TickerError>;
        }
    }

    impl CrossBeamTicker {
        pub fn duration(&self) -> Duration {
            self.duration
        }
    }

    #[test]
    fn ticker_stop() {
        let ticker = CrossBeamTicker::new(Duration::from_millis(1));

        // wait of first ticker fire
        assert!(ticker.next().is_ok(), "ticker could not be fired");

        // cancel the ticker
        ticker.stop().unwrap();
        assert!(ticker.next().is_err(), "ticker was not cancelled")
    }
}
