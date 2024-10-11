pub mod cli;
pub mod crowdstrike;
pub mod http_client;

#[derive(Debug, Clone)]
pub struct Installer {
    pub os: String,
    pub os_version: String,
    pub sha256: String,
}
