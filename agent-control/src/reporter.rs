//! Module containing the different reporters that, when self-instrumentation is enabled, can be
//! started to report additional data, possibly periodically.
#![warn(missing_docs)]

mod uptime;

pub use uptime::{UptimeReporter, UptimeReporterInterval};
