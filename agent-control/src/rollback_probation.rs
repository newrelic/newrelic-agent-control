//! The rollback probation module manages the state of the agent's boot status, particularly in
//! scenarios where the agent may be experiencing crashes or instability. It tracks the number of
//! consecutive crashes, the current and previous versions of the agent, and determines when to
//! trigger a rollback to a previous stable version.
use serde::{Deserialize, Serialize};
use std::{
    env,
    error::Error,
    fs,
    io::BufReader,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const AGENT_CONTROL_BOOT_DATA_FILE: &str = "agent_control_boot_data.json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum BootStatus {
    // Everything's fine, let's continue!
    Stable,
    // Should check with retries
    #[default]
    Validating,
}

impl PartialEq<BootStatus> for &BootStatus {
    fn eq(&self, other: &BootStatus) -> bool {
        *self == other
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct BootData {
    status: BootStatus,
    current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_path: Option<PathBuf>,
    n_attempts: usize,
    #[serde(default)]
    last_crash_timestamp: u64,
}

impl Default for BootData {
    fn default() -> Self {
        BootData {
            status: BootStatus::default(),
            current_version: CURRENT_VERSION.to_string(),
            previous_version: None,
            backup_path: None,
            n_attempts: 0,
            last_crash_timestamp: 0,
        }
    }
}

impl BootData {
    pub fn set_status(self, status: BootStatus) -> Self {
        BootData { status, ..self }
    }

    pub fn set_backup_path(self, backup_path: Option<PathBuf>) -> Self {
        BootData {
            backup_path,
            ..self
        }
    }

    pub fn set_previous_version(self, previous_version: Option<String>) -> Self {
        BootData {
            previous_version,
            ..self
        }
    }

    pub fn status(&self) -> &BootStatus {
        &self.status
    }

    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    pub fn previous_version(&self) -> Option<&String> {
        self.previous_version.as_ref()
    }

    pub fn backup_path(&self) -> Option<&PathBuf> {
        self.backup_path.as_ref()
    }

    pub fn n_attempts(&self) -> usize {
        self.n_attempts
    }

    pub fn increment_crash_count(mut self) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // If version changed, treats as new probation
        if self.current_version != CURRENT_VERSION {
            self.n_attempts = 1;
            self.current_version = CURRENT_VERSION.to_string();
            self.last_crash_timestamp = now;
            return self;
        }

        // If last crash was long ago (e.g. > 3600 seconds), reset count
        // This handles "flaky but stable" scenarios where uptime is long enough.
        if now.saturating_sub(self.last_crash_timestamp) > 3600 {
            self.n_attempts = 1;
        } else {
            self.n_attempts += 1;
        }
        self.last_crash_timestamp = now;
        self
    }

    pub fn should_trigger_rollback(&self) -> bool {
        self.status == BootStatus::Validating && self.n_attempts >= 3
    }
}

pub fn retrieve_rollback_probation_data() -> Option<BootData> {
    let cur_dir = env::current_dir().ok()?;
    let boot_data_file = cur_dir.join(AGENT_CONTROL_BOOT_DATA_FILE);
    let boot_data_file = fs::File::open(boot_data_file).ok()?;
    let boot_data_reader = BufReader::new(boot_data_file);
    serde_json::from_reader(boot_data_reader).ok()
}

pub fn persist_rollback_probation_data(data: &BootData) -> Result<(), Box<dyn Error>> {
    let cur_dir = env::current_dir()?;
    let boot_data_file = cur_dir.join(AGENT_CONTROL_BOOT_DATA_FILE);
    let serialized_data = serde_json::to_string_pretty(&data)?;
    Ok(fs::write(boot_data_file, serialized_data)?)
}
