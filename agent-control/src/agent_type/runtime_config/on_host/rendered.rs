use crate::agent_type::runtime_config::on_host::package::rendered::Package;
use crate::agent_type::runtime_config::{
    health_config::rendered::OnHostHealthConfig,
    on_host::{executable::rendered::Executable, filesystem::rendered::FileSystem},
    version_config::rendered::OnHostVersionConfig,
};
use std::collections::HashMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct OnHost {
    pub executables: Vec<Executable>,
    pub enable_file_logging: bool,
    /// Enables and define health checks configuration.
    pub health: OnHostHealthConfig,
    pub version: Option<OnHostVersionConfig>,
    pub filesystem: FileSystem,
    pub packages: HashMap<String, Package>,
}
