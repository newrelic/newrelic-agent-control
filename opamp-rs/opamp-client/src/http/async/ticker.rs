//! Interface and implementation of an asynchronous ticker that can be reset and stopped.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::{
    select,
    sync::{
        mpsc::{channel, error::SendError, Receiver, Sender},
        Mutex,
    },
    time::interval,
};

/// The size of the channel buffer.
static CHANNEL_BUFFER: usize = 1;

/// The error enum for Ticker errors.
#[derive(Debug, Error)]
pub enum AsyncTickerError {
    /// Error variant indicating that the ticker is cancelled.
    #[error("ticker cancelled")]
    Cancelled,

    /// Error variant for SendError with associated TickerEvent.
    #[error("`{0}`")]
    SendError(#[from] SendError<AsyncTickerEvent>),
}

/// The AsyncTicker trait defining the asynchronous functions `next`, `reset`, and `stop`.
#[async_trait]
pub trait AsyncTicker {
    /// Returns the unit value if a tick was fired. Returns an error if the channel is closed.
    async fn next(&self) -> Result<(), AsyncTickerError>;

    /// Reset the ticker. Returns an error if unable to reset.
    async fn reset(&self) -> Result<(), AsyncTickerError>;

    /// Stop the ticker. Returns an error if unable to stop.
    async fn stop(&self) -> Result<(), AsyncTickerError>;
}

/// The events that control the behavior of the ticker.
pub enum AsyncTickerEvent {
    /// Event to reset the ticker.
    Reset,

    /// Event to stop the ticker.
    Stop,
}

/// Tokio-based AsyncTicker implementation.
pub struct TokioTicker {
    /// The duration between ticks.
    duration: Duration,

    /// The receiver for receiving reset and stop events.
    reset_receiver: Arc<Mutex<Receiver<AsyncTickerEvent>>>,

    /// The sender for sending reset and stop events.
    reset_sender: Sender<AsyncTickerEvent>,
}

impl TokioTicker {
    /// Construct a new TokioTicker with the specified duration.
    ///
    /// # Arguments
    ///
    /// * `duration` - The duration between ticks.
    pub(super) fn new(duration: Duration) -> Self {
        let (reset_sender, reset_receiver) = channel(CHANNEL_BUFFER);
        Self {
            duration,
            reset_receiver: Arc::new(Mutex::new(reset_receiver)),
            reset_sender,
        }
    }
}

#[async_trait]
impl AsyncTicker for TokioTicker {
    /// Wait for ticks and channel messages that will reset the ticker.
    /// If the channel is closed, it will return AsyncTickerError::Cancelled.
    async fn next(&self) -> Result<(), AsyncTickerError> {
        let mut ticker = interval(self.duration);

        // First ticker interval is fired instantaneously.
        ticker.tick().await;

        let mut reset_receiver = self.reset_receiver.lock().await;
        select! {
            biased;

            reset_result = reset_receiver.recv() => match reset_result {
                Some(event) => match event {
                    AsyncTickerEvent::Reset => ticker.reset(),
                    AsyncTickerEvent::Stop => return Err(AsyncTickerError::Cancelled),
                },
                None => return Err(AsyncTickerError::Cancelled),
            },
            _ = ticker.tick() => {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Reset the ticker.
    async fn reset(&self) -> Result<(), AsyncTickerError> {
        self.reset_sender.send(AsyncTickerEvent::Reset).await?;
        Ok(())
    }

    /// Stop the ticker.
    async fn stop(&self) -> Result<(), AsyncTickerError> {
        self.reset_sender.send(AsyncTickerEvent::Stop).await?;
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod test {
    use super::*;
    use mockall::mock;

    // Ticker mock for testing purposes.
    mock! {
          pub(crate) TickerMockAll {}


        #[async_trait]
        impl AsyncTicker for TickerMockAll {
            async fn next(&self) -> Result<(), AsyncTickerError>;
            async fn reset(&self) -> Result<(), AsyncTickerError>;
            async fn stop(&self) -> Result<(), AsyncTickerError>;
        }
    }

    impl TokioTicker {
        pub fn duration(&self) -> Duration {
            self.duration
        }
    }

    #[tokio::test]
    async fn tokio_ticker_stop() {
        let ticker = TokioTicker::new(Duration::from_millis(1));

        // wait of first ticker fire
        assert!(ticker.next().await.is_ok(), "ticker could not be fired");

        // cancel the ticker
        ticker.stop().await.unwrap();
        assert!(ticker.next().await.is_err(), "ticker was not cancelled")
    }
}
