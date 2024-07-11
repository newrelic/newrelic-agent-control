use std::path::Path;

cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-super-agent";
        const SUPER_AGENT_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-super-agent";
        const SUPER_AGENT_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-super-agent";

    } else {
        const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/etc/newrelic-super-agent";
        const SUPER_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent";
        const SUPER_AGENT_LOG_DIR: &str = "/var/log/newrelic-super-agent";
    }
}

// TODO: remove the corresponding values from `defaults.rs` when all references are removed.
pub struct SuperAgentPaths {
    local_data_dir: String,
    data_dir: String, // TODO: should we rename to `remote_dir`?
    log_dir: String,
}

impl Default for SuperAgentPaths {
    fn default() -> Self {
        Self::new(
            SUPER_AGENT_LOCAL_DATA_DIR,
            SUPER_AGENT_DATA_DIR,
            SUPER_AGENT_LOG_DIR,
        )
    }
}

impl SuperAgentPaths {
    fn new(local_data_dir: &str, data_dir: &str, log_dir: &str) -> Self {
        Self {
            local_data_dir: local_data_dir.to_string(),
            data_dir: data_dir.to_string(),
            log_dir: log_dir.to_string(),
        }
    }

    // TODO: check  if directories should be set separately
    #[cfg(debug_assertions)]
    pub fn with_path(path: &Path) -> Self {
        let local_data_dir = path.join("nrsa_local").to_string_lossy().to_string();
        let data_dir = path.join("nrsa_remote").to_string_lossy().to_string();
        let log_dir = path.join("nrsa_logs").to_string_lossy().to_string();

        Self::new(&local_data_dir, &data_dir, &log_dir)
    }

    pub fn local_data_dir(&self) -> &str {
        &self.local_data_dir
    }

    pub fn data_dir(&self) -> &str {
        &self.data_dir
    }

    pub fn log_dir(&self) -> &str {
        &self.log_dir
    }
}
