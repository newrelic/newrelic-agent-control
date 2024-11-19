//! # Synchronous HTTP Client implementation for OpAMP.
pub mod client;
pub mod http_client;
mod managed_client;
mod sender;
mod ticker;

// export public structs
pub use {
    http_client::HttpClientError, http_client::HttpClientUreq,
    managed_client::NotStartedHttpClient, managed_client::StartedHttpClient, ticker::Ticker,
    ticker::TickerError,
};
