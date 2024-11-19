//! HTTP client implementations.

pub(super) mod client;
mod managed_client;
pub(super) mod sender;

mod http_client;
mod ticker;

// export public structs
pub use {
    http_client::AsyncHttpClientError, http_client::HttpClientReqwest,
    managed_client::AsyncNotStartedHttpClient, managed_client::AsyncStartedHttpClient,
    ticker::AsyncTicker, ticker::AsyncTickerError,
};
