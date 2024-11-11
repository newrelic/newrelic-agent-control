pub mod agent_type;
pub mod cli;
pub mod context;
pub mod event;
pub mod http;
pub mod logging;
pub mod opamp;
pub mod sub_agent;
pub mod super_agent;
pub mod utils;

pub mod values;

#[cfg(feature = "k8s")]
pub mod k8s;

#[cfg(feature = "onhost")]
pub mod config_migrate;

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");
