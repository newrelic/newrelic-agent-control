pub mod collection;
pub mod effective_agents_assembler;
pub mod error;
mod event_handler;
pub mod event_processor;
pub mod event_processor_builder;
pub mod health;
pub mod persister;

#[cfg(feature = "k8s")]
pub mod k8s;
#[cfg(feature = "onhost")]
pub mod on_host;

pub use sub_agent::*;
#[allow(clippy::module_inception)]
mod sub_agent;
