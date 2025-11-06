use crate::agent_control::defaults::{
    AGENT_CONTROL_LOCAL_DATA_DIR, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use crate::cli::error::CliError;
use crate::on_host::file_store::build_config_name;
use std::path::PathBuf;
use tracing::info;

/// Represents the values to create or migrate an infra-config
pub struct OtelConfigGen {
    otel_agent_values_path: PathBuf,
    otel_config_source_path: PathBuf,
}

impl Default for OtelConfigGen {
    fn default() -> Self {
        Self {
            otel_agent_values_path: PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR)
                .join(FOLDER_NAME_LOCAL_DATA)
                .join("nrdot"),
            otel_config_source_path: PathBuf::from(
                "/etc/newrelic-agent-control/examples/values-nr-otel-collector-agent-linux.yaml",
            ),
        }
    }
}

impl OtelConfigGen {
    /// generate_otel_config is gathering the embedded otel values file that is downloaded on build
    /// time from the GitHub repository using the pinned version from the Goreleaser file.
    /// Once copied the limit_mib is modified.
    pub fn generate_otel_config(&self) -> Result<(), CliError> {
        info!("Generating otel configuration");
        self.create_directories()?;
        self.modify_values_yaml()?;
        info!("Local otel config file successfully created");
        Ok(())
    }

    fn create_directories(&self) -> Result<(), CliError> {
        std::fs::create_dir_all(self.otel_agent_values_path.clone()).map_err(|err| {
            CliError::Command(format!("error creating otel values directory: {err}"))
        })?;
        Ok(())
    }

    fn modify_values_yaml(&self) -> Result<(), CliError> {
        let source_path = self.otel_config_source_path.clone();
        let file_path = self
            .otel_agent_values_path
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG));
        let content = std::fs::read_to_string(source_path)
            .map_err(|err| CliError::Command(format!("error reading otel values file: {err}")))?;

        let modified_content = content
            .lines()
            .map(|line| {
                if line.starts_with("limit_mib:") {
                    "limit_mib: 100".to_string()
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        std::fs::write(file_path, modified_content)
            .map_err(|err| CliError::Command(format!("error writing otel values file: {err}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::defaults::{FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG};
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    impl OtelConfigGen {
        fn new(otel_agent_values_path: &str, otel_config_source_path: &str) -> OtelConfigGen {
            Self {
                otel_agent_values_path: PathBuf::from(otel_agent_values_path),
                otel_config_source_path: PathBuf::from(otel_config_source_path),
            }
        }
    }

    #[test]
    fn test_generate_otel_config_creates_directories_and_copies_file() {
        let temp_dir = tempdir().unwrap();
        let temp_values_dir = temp_dir.path().join(FOLDER_NAME_LOCAL_DATA).join("nrdot");
        let temp_example_file = temp_dir
            .path()
            .join("values-nr-otel-collector-agent-linux.yaml");

        let _ = fs::create_dir_all(temp_dir.path().join("examples"));
        let mut file = fs::File::create(&temp_example_file).unwrap();
        let _ = writeln!(file, "limit_mib: 50\nOTHER_CONFIG: value");

        let otel_config_gen = OtelConfigGen::new(
            temp_values_dir.to_str().unwrap(),
            temp_example_file.to_str().unwrap(),
        );
        let result = otel_config_gen.generate_otel_config();
        assert!(result.is_ok());
        assert!(temp_values_dir.exists());

        let values_file = temp_values_dir.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG));
        let values_content = fs::read_to_string(&values_file).unwrap();
        assert!(values_content.contains("limit_mib: 100"));
        assert!(values_content.contains("OTHER_CONFIG: value"));
    }
}
