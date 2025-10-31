//! Implementation of the generate-config command for the on-host cli.

use crate::cli::error::CliError;
use crate::cli::on_host::config_gen::region::{Region, region_parser};
use std::fs::OpenOptions;
use std::io::Write;
use tracing::info;

const CONFIG_PATH: &str = "/etc/newrelic-agent-control/newrelic-agent-control.conf";
const NEW_RELIC_LICENSE_CONFIG_KEY: &str = "NEW_RELIC_LICENSE_KEY";
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

/// Generates the entries required by agent-control in newrelic-agent-control.conf.
pub fn generate_systemd_config(args: Args) -> Result<(), CliError> {
    info!("Adding required values to newrelic-agent-control.conf ");

    update_newrelic_license_key(CONFIG_PATH, &args.newrelic_license_key)?;

    update_otel_exporter_endpoint(CONFIG_PATH, args.region)?;

    info!("Host monitoring values generated successfully");
    Ok(())
}

fn update_newrelic_license_key(config_path: &str, new_license_key: &str) -> Result<(), CliError> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|err| CliError::Command(format!("error reading agent control .conf file: {err}")))?
        .lines()
        .filter(|line| !line.starts_with(NEW_RELIC_LICENSE_CONFIG_KEY))
        .collect::<Vec<_>>()
        .join("\n");

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(config_path)
        .map_err(|err| {
            CliError::Command(format!("error opening agent control .conf file: {err}"))
        })?;

    let new_content = format!(
        "{}\n{}=\"{}\"\n",
        content, NEW_RELIC_LICENSE_CONFIG_KEY, new_license_key
    );
    file.write_all(new_content.as_bytes()).map_err(|err| {
        CliError::Command(format!("error updating agent control .conf file: {err}"))
    })?;

    Ok(())
}

fn update_otel_exporter_endpoint(config_path: &str, region: Region) -> Result<(), CliError> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|err| CliError::Command(format!("error reading agent control .conf file: {err}")))?
        .lines()
        .filter(|line| !line.starts_with(OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY))
        .collect::<Vec<_>>()
        .join("\n");

    let new_content = format!(
        "{}\n{}=https://{}:4317/\n",
        content,
        OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY,
        region.otel_endpoint()
    );

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(config_path)
        .map_err(|err| {
            CliError::Command(format!("error opening agent control .conf file: {err}"))
        })?;

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
    fn test_update_newrelic_license_key() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("newrelic-agent-control.conf");

        let initial_content =
            NEW_RELIC_LICENSE_CONFIG_KEY.to_string() + "=\"old_key\"\nOTHER_CONFIG=\"value\"";
        std::fs::write(&file_path, initial_content).unwrap();

        let result = update_newrelic_license_key(file_path.to_str().unwrap(), "new_key");
        assert!(result.is_ok());

        let updated_content = std::fs::read_to_string(&file_path).unwrap();
        assert!(updated_content.contains(&format!("{}=\"new_key\"", NEW_RELIC_LICENSE_CONFIG_KEY)));
        assert!(
            !updated_content.contains(&format!("{}=\"old_key\"", NEW_RELIC_LICENSE_CONFIG_KEY))
        );
        assert!(updated_content.contains("OTHER_CONFIG=\"value\""));
    }

    #[test]
    fn test_update_otel_exporter_endpoint() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("newrelic-agent-control.conf");

        let initial_content = format!(
            "{}=\"old_endpoint\"\nOTHER_CONFIG=\"value\"",
            OTEL_EXPORTER_OTLP_ENDPOINT_CONFIG_KEY
        );
        std::fs::write(&file_path, initial_content).unwrap();

        let result = update_otel_exporter_endpoint(file_path.to_str().unwrap(), Region::EU);
        assert!(result.is_ok());

        let updated_content = std::fs::read_to_string(&file_path).unwrap();
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
