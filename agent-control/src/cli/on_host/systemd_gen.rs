//! Implementation of the generate-config command for the on-host cli.

use crate::agent_control::defaults::AGENT_CONTROL_LOCAL_DATA_DIR;
use crate::cli::error::CliError;
use crate::cli::on_host::config_gen::region::{Region, region_parser};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::info;

pub const SERVICE_CONFIG_FILE: &str = "systemd-env.conf";
pub const NEW_RELIC_LICENSE_CONFIG_KEY: &str = "NEW_RELIC_LICENSE_KEY";
const OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// Generates the Agent Control configuration for host environments.
#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Sets which host monitoring source to be used.
    #[arg(long, required = true)]
    newrelic_license_key: String,

    /// New Relic region
    #[arg(long, value_parser = region_parser(), required = true)]
    region: Region,
}

/// Generates the entries required by agent-control in [SERVICE_CONFIG_FILE].
pub fn generate_systemd_config(args: Args) -> Result<(), CliError> {
    info!("Adding required values to newrelic-agent-control.conf ");

    let config_path = PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR).join(SERVICE_CONFIG_FILE);

    update_config(
        config_path.as_path(),
        &args.newrelic_license_key,
        args.region,
    )?;

    info!("Host monitoring values generated successfully");
    Ok(())
}

fn update_config(
    config_path: &Path,
    new_license_key: &str,
    region: Region,
) -> Result<(), CliError> {
    // Read the content from the configuration file
    let content = std::fs::read_to_string(config_path)
        .map_err(|err| CliError::Command(format!("error reading agent control .conf file: {err}")))?
        .lines()
        .filter(|line| {
            !line.starts_with(NEW_RELIC_LICENSE_CONFIG_KEY)
                && !line.starts_with(OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Prepare the new content with updated license key and OTEL endpoint
    let new_content = format!(
        "{}\n{}=\"{}\"\n{}=https://{}:4317/\n",
        content,
        NEW_RELIC_LICENSE_CONFIG_KEY,
        new_license_key,
        OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY,
        region.otel_endpoint()
    );

    // Open the file for writing and truncate it
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(config_path)
        .map_err(|err| {
            CliError::Command(format!("error opening agent control .conf file: {err}"))
        })?;

    // Write the new content to the file
    file.write_all(new_content.as_bytes()).map_err(|err| {
        CliError::Command(format!("error updating agent control .conf file: {err}"))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_update_config() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("newrelic-agent-control.conf");

        let initial_content = format!(
            "{}=\"old_key\"\n{}=\"old_endpoint\"\nOTHER_CONFIG=\"value\"",
            NEW_RELIC_LICENSE_CONFIG_KEY, OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY
        );
        std::fs::write(&file_path, initial_content).unwrap();

        let new_license_key = "new_key";
        let region = Region::EU; // Assuming Region::EU is a valid variant

        let result = update_config(file_path.as_path(), new_license_key, region);
        assert!(result.is_ok());

        let updated_content = std::fs::read_to_string(&file_path).unwrap();

        assert!(updated_content.contains(&format!(
            "{}=\"{}\"",
            NEW_RELIC_LICENSE_CONFIG_KEY, new_license_key
        )));
        assert!(
            !updated_content.contains(&format!("{}=\"old_key\"", NEW_RELIC_LICENSE_CONFIG_KEY))
        );

        assert!(updated_content.contains(&format!(
            "{}=https://otlp.eu01.nr-data.net:4317/",
            OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY
        )));
        assert!(!updated_content.contains(&format!(
            "{}=\"old_endpoint\"",
            OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY
        )));

        assert!(updated_content.contains("OTHER_CONFIG=\"value\""));
    }
}
