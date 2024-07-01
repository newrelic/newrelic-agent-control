pub mod auth;
pub mod callbacks;
pub mod client_builder;
pub mod effective_config;
pub mod hash_repository;
pub mod http;
pub mod instance_id;
pub mod operations;
pub mod remote_config;
pub mod remote_config_hash;
pub mod remote_config_report;

pub type LastErrorCode = u16;
pub type LastErrorMessage = String;
