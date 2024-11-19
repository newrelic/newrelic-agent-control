//! OpAMP client library.

#![warn(missing_docs)]

// public exported traits
pub(crate) mod common;
pub mod operation;

// OpAMP protobuffers module files will be excluded from documentation.
#[doc(hidden)]
#[allow(unknown_lints)]
#[allow(clippy::mixed_attributes_style)]
pub mod opamp {
    //! The opamp module contains all those entities defined by the
    //! [Opamp specification](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md)
    pub mod proto {
        //! The proto module contains the protobuffers structures defined by the
        //! [Opamp specification](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md)
        include!(concat!("../../proto", "/opamp.proto.rs"));
        include!(concat!("../../proto", "/debug.rs"));
    }
}

pub mod error;
pub use error::{NotStartedClientError, NotStartedClientResult};

pub mod http;

#[cfg(feature = "async")]
pub mod async_client;
#[cfg(feature = "async")]
pub use async_client::*;

#[cfg(feature = "async")]
pub mod async_error;
#[cfg(feature = "async")]
pub use async_error::*;

// Sync feature
#[cfg(feature = "sync")]
pub mod sync_client;
#[cfg(feature = "sync")]
pub use sync_client::*;

#[cfg(feature = "sync")]
pub mod sync_error;
#[cfg(feature = "sync")]
pub use sync_error::*;
