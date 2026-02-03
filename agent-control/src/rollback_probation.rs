use serde::{Deserialize, Serialize};
use std::{env, error::Error, fs, io::BufReader, path::PathBuf};

#[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum BootStatus {
    // Everything's fine, let's continue!
    Stable,
    // Should check with retries
    #[default]
    Validating,
    // Irrecoverable error, rollback required
    Failed,
}
#[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct BootData {
    status: BootStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_path: Option<PathBuf>,
    n_attempts: usize,
}

impl BootData {
    pub fn set_status(self, status: BootStatus) -> Self {
        BootData { status, ..self }
    }

    pub fn status(&self) -> &BootStatus {
        &self.status
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

    pub fn increment_crash_count(self) -> Self {
        BootData {
            n_attempts: self.n_attempts + 1,
            ..self
        }
    }

    pub fn should_trigger_rollback(&self) -> bool {
        self.status == BootStatus::Failed
            || (self.status == BootStatus::Validating && self.n_attempts >= 3)
    }
}

pub fn retrieve_rollback_probation_data() -> Option<BootData> {
    let cur_dir = env::current_dir().ok()?;
    let boot_data_file = cur_dir.join("agent_control_boot_data.json");
    let boot_data_file = fs::File::open(boot_data_file).ok()?;
    let boot_data_reader = BufReader::new(boot_data_file);
    serde_json::from_reader(boot_data_reader).ok()
}

pub fn persist_rollback_probation_data(data: BootData) -> Result<(), Box<dyn Error>> {
    let cur_dir = env::current_dir()?;
    let boot_data_file = cur_dir.join("agent_control_boot_data.json");
    let serialized_data = serde_json::to_string_pretty(&data)?;
    Ok(fs::write(boot_data_file, serialized_data)?)
}
