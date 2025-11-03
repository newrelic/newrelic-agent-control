use crate::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA, INSTANCE_ID_FILENAME,
    STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG,
};
use crate::cli::error::CliError;
use crate::opamp::instance_id::on_host::storer::build_config_name;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error};

const LOCAL_DATA_DIR: &str = "/etc/newrelic-agent-control";
const REMOTE_DATA_DIR: &str = "/var/lib/newrelic-agent-control";
const OLD_ENV_FILE_NAME: &str = "newrelic-agent-control.conf";
const NEW_ENV_FILE_NAME: &str = "systemd-env.conf";
const OTEL_AGENT_ID: &str = "nrdot";
const VALUES_FOLDER: &str = "values";
const INFRA_AGENT_ID: &str = "nr-infra";
// old folder and file names
const OLD_CONFIG_AGENT_CONTROL_FILE_NAME: &str = "config.yaml";
const OLD_IDENTIFIERS_YAML: &str = "identifiers.yaml";
const OLD_CONFIG_SUB_AGENT_FILE_NAME: &str = "values.yaml";
const OLD_SUB_AGENT_DATA_DIR: &str = "fleet/agents.d";

/// TODO: TEMPORAL SCRIPT TO MIGRATE PATHS AND NAMES AFTER SOME TIME THIS SHOULD BE DELETED
pub fn migrate() -> Result<(), CliError> {
    let local_base = PathBuf::from(LOCAL_DATA_DIR);
    let remote_base = PathBuf::from(REMOTE_DATA_DIR);

    let new_local_data_path = local_base.join(FOLDER_NAME_LOCAL_DATA);

    // Check if the new folder already exists - local-data
    if new_local_data_path.exists() && new_local_data_path.is_dir() {
        move_and_rename(&local_base, &remote_base)?;
    }
    Ok(())
}

// Copy the old files in the new paths but leaving the old ones in place
fn move_and_rename(local_base: &Path, remote_base: &Path) -> Result<(), CliError> {
    debug!("Starting migration: moving files from old structure to new structure...");

    let migration_pairs = get_migration_list(local_base, remote_base);

    for (old_path, new_path) in &migration_pairs {
        if let Some(parent_dir) = new_path.parent() {
            if !parent_dir.exists() {
                let dir_display = parent_dir.display();
                debug!("Destination directory '{dir_display}' does not exist, creating it.",);

                if let Err(e) = fs::create_dir_all(parent_dir) {
                    let msg =
                        format!("Failed to create destination directory '{dir_display}': {e}");
                    error!(msg);
                    return Err(CliError::FileSystemError(msg));
                }
            }
        } else {
            let path_display = new_path.display();
            let msg = format!("Invalid destination path structure: {path_display}");
            error!(msg);
            return Err(CliError::FileSystemError(msg));
        }

        copy_and_rename_item(old_path, new_path)?;
    }

    debug!("Migration: all steps completed.");
    Ok(())
}

fn copy_and_rename_item(old_path: &Path, new_path: &Path) -> Result<(), CliError> {
    if old_path.exists() {
        let old_path_display = old_path.display();
        let new_path_display = new_path.display();

        debug!("Copying '{old_path_display}' to '{new_path_display}'",);

        if let Err(e) = fs::copy(old_path, new_path) {
            let msg = format!("Failed to copy '{old_path_display}' to '{new_path_display}': {e}");
            error!(msg);
            return Err(CliError::FileSystemError(msg));
        }
    }
    Ok(())
}
fn add_agent_control_files(
    migration_pairs: &mut Vec<(PathBuf, PathBuf)>,
    local_base: &Path,
    remote_base: &Path,
) {
    // --- LOCAL ---
    // agent control config.yaml -> local-data/agent-control/local_config.yaml
    migration_pairs.push((
        local_base.join(OLD_CONFIG_AGENT_CONTROL_FILE_NAME),
        local_base
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
    ));
    // agent-control-config.conf -> systemd-env.conf
    migration_pairs.push((
        local_base.join(OLD_ENV_FILE_NAME),
        local_base.join(NEW_ENV_FILE_NAME),
    ));

    // --- REMOTE ---
    // agent control config.yaml -> fleet-data/agent-control/remote_config.yaml
    migration_pairs.push((
        remote_base.join(OLD_CONFIG_AGENT_CONTROL_FILE_NAME),
        remote_base
            .join(FOLDER_NAME_FLEET_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG)),
    ));
    // agent control identifiers.yaml -> fleet-data/agent-control/instance_id.yaml
    migration_pairs.push((
        remote_base.join(OLD_IDENTIFIERS_YAML),
        remote_base
            .join(FOLDER_NAME_FLEET_DATA)
            .join(AGENT_CONTROL_ID)
            .join(INSTANCE_ID_FILENAME),
    ));
}

fn add_infra_agent_files(
    migration_pairs: &mut Vec<(PathBuf, PathBuf)>,
    local_base: &Path,
    remote_base: &Path,
) {
    // --- LOCAL ---
    let old_local_infra_dir = local_base.join(OLD_SUB_AGENT_DATA_DIR).join(INFRA_AGENT_ID);
    if old_local_infra_dir.exists() && old_local_infra_dir.is_dir() {
        debug!(
            "Found old local nr-infra directory, adding to migration: {}",
            old_local_infra_dir.display()
        );
        // nf-infra values.yaml -> local-data/nr-infra/local_config.yaml
        migration_pairs.push((
            old_local_infra_dir
                .join(VALUES_FOLDER)
                .join(OLD_CONFIG_SUB_AGENT_FILE_NAME),
            local_base
                .join(FOLDER_NAME_LOCAL_DATA)
                .join(INFRA_AGENT_ID)
                .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        ));
    }

    // --- REMOTE  ---
    let old_remote_infra_dir = remote_base
        .join(OLD_SUB_AGENT_DATA_DIR)
        .join(INFRA_AGENT_ID);
    if old_remote_infra_dir.exists() && old_remote_infra_dir.is_dir() {
        debug!(
            "Found old remote nr-infra directory, adding to migration: {}",
            old_remote_infra_dir.display()
        );
        // nr-infra values.yaml -> fleet-data/nr-infra/remote_config.yaml
        migration_pairs.push((
            old_remote_infra_dir
                .join(VALUES_FOLDER)
                .join(OLD_CONFIG_SUB_AGENT_FILE_NAME),
            remote_base
                .join(FOLDER_NAME_FLEET_DATA)
                .join(INFRA_AGENT_ID)
                .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG)),
        ));
        // nr-infra identifiers.yaml -> fleet-data/nr-infra/instance_id.yaml
        migration_pairs.push((
            old_remote_infra_dir.join(OLD_IDENTIFIERS_YAML),
            remote_base
                .join(FOLDER_NAME_FLEET_DATA)
                .join(INFRA_AGENT_ID)
                .join(INSTANCE_ID_FILENAME),
        ));
    }
}

fn add_otel_agent_files(
    migration_pairs: &mut Vec<(PathBuf, PathBuf)>,
    local_base: &Path,
    remote_base: &Path,
) {
    // --- LOCAL ---
    let old_local_otel_dir = local_base.join(OLD_SUB_AGENT_DATA_DIR).join(OTEL_AGENT_ID);
    if old_local_otel_dir.exists() && old_local_otel_dir.is_dir() {
        debug!(
            "Found old local nrdot directory, adding to migration: {}",
            old_local_otel_dir.display()
        );
        // nrdot values.yaml -> local-data/nrdot/local_config.yaml
        migration_pairs.push((
            old_local_otel_dir
                .join(VALUES_FOLDER)
                .join(OLD_CONFIG_SUB_AGENT_FILE_NAME),
            local_base
                .join(FOLDER_NAME_LOCAL_DATA)
                .join(OTEL_AGENT_ID)
                .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        ));
    }

    // --- REMOTE ---
    let old_remote_otel_dir = remote_base.join(OLD_SUB_AGENT_DATA_DIR).join(OTEL_AGENT_ID);
    if old_remote_otel_dir.exists() && old_remote_otel_dir.is_dir() {
        debug!(
            "Found old remote nrdot directory, adding to migration: {}",
            old_remote_otel_dir.display()
        );
        // nrdot values.yaml -> fleet-data/nrdot/remote_config.yaml
        migration_pairs.push((
            old_remote_otel_dir
                .join(VALUES_FOLDER)
                .join(OLD_CONFIG_SUB_AGENT_FILE_NAME),
            remote_base
                .join(FOLDER_NAME_FLEET_DATA)
                .join(OTEL_AGENT_ID)
                .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG)),
        ));
        // nrdot identifiers.yaml -> fleet-data/nrdot/instance_id.yaml
        migration_pairs.push((
            old_remote_otel_dir.join(OLD_IDENTIFIERS_YAML),
            remote_base
                .join(FOLDER_NAME_FLEET_DATA)
                .join(OTEL_AGENT_ID)
                .join(INSTANCE_ID_FILENAME),
        ));
    }
}

fn get_migration_list(local_base: &Path, remote_base: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut migration_pairs = Vec::new();

    add_agent_control_files(&mut migration_pairs, local_base, remote_base);
    add_infra_agent_files(&mut migration_pairs, local_base, remote_base);
    add_otel_agent_files(&mut migration_pairs, local_base, remote_base);

    migration_pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::tempdir;

    #[test]
    fn test_move_item_success() {
        let temp_dir = tempdir().unwrap();
        let old_path = temp_dir.path().join("old.txt");
        let new_path = temp_dir.path().join("new.txt");

        File::create(&old_path).unwrap();

        assert!(old_path.exists());
        assert!(!new_path.exists());

        let result = copy_and_rename_item(&old_path, &new_path);
        assert!(result.is_ok());

        assert!(
            old_path.exists(),
            "The old file should still exist after copy"
        );
        assert!(new_path.exists(), "The new file should exist");
    }

    #[test]
    fn test_move_item_skips_if_not_exists() {
        let temp_dir = tempdir().unwrap();
        let old_path = temp_dir.path().join("non_existent.txt");
        let new_path = temp_dir.path().join("new.txt");

        assert!(!old_path.exists());

        let result = copy_and_rename_item(&old_path, &new_path);
        assert!(result.is_ok());

        assert!(
            !new_path.exists(),
            "The new file should not have been created"
        );
    }

    #[test]
    fn test_full_migration_logic_with_all_agents() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();

        let local_base = root.join("etc");
        let remote_base = root.join("var");
        fs::create_dir_all(&local_base).unwrap();
        fs::create_dir_all(&remote_base).unwrap();

        fs::create_dir_all(local_base.join(OLD_SUB_AGENT_DATA_DIR).join(INFRA_AGENT_ID)).unwrap();
        fs::create_dir_all(local_base.join(OLD_SUB_AGENT_DATA_DIR).join(OTEL_AGENT_ID)).unwrap();
        fs::create_dir_all(
            remote_base
                .join(OLD_SUB_AGENT_DATA_DIR)
                .join(INFRA_AGENT_ID),
        )
        .unwrap();
        fs::create_dir_all(remote_base.join(OLD_SUB_AGENT_DATA_DIR).join(OTEL_AGENT_ID)).unwrap();

        let migration_pairs = get_migration_list(&local_base, &remote_base);
        assert_eq!(
            migration_pairs.len(),
            10,
            "Migration list should contain all 10 items when old agent dirs exist"
        );

        for (old_path, _) in migration_pairs.iter() {
            let parent = old_path.parent().unwrap();
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                panic!(
                    "Failed to create old parent dir {}: {}",
                    parent.display(),
                    e
                )
            });
            File::create(old_path).unwrap_or_else(|e| {
                panic!("Failed to create old file {}: {}", old_path.display(), e)
            });
        }

        let result = move_and_rename(&local_base, &remote_base);
        assert!(result.is_ok());

        for (old_path, new_path) in migration_pairs.iter() {
            assert!(
                old_path.exists(),
                "Old file {} should still exist after copy",
                old_path.display()
            );
            assert!(
                new_path.exists(),
                "New file {} was not created",
                new_path.display()
            );
        }
    }

    #[test]
    fn test_migration_logic_with_no_sub_agents() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();

        let local_base = root.join("etc");
        let remote_base = root.join("var");
        fs::create_dir_all(&local_base).unwrap();
        fs::create_dir_all(&remote_base).unwrap();

        let migration_pairs = get_migration_list(&local_base, &remote_base);
        assert_eq!(
            migration_pairs.len(),
            4,
            "Migration list should only contain 4 (agent-control) items when no sub-agent dirs exist"
        );

        for (old_path, _) in migration_pairs.iter() {
            let parent = old_path.parent().unwrap();
            fs::create_dir_all(parent).unwrap();
            File::create(old_path).unwrap();
        }

        let result = move_and_rename(&local_base, &remote_base);
        assert!(result.is_ok());

        for (old_path, new_path) in migration_pairs.iter() {
            assert!(old_path.exists());
            assert!(new_path.exists());
        }
    }

    #[test]
    fn test_migration_logic_only_remote_infra() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();

        let local_base = root.join("etc");
        let remote_base = root.join("var");
        fs::create_dir_all(&local_base).unwrap();
        fs::create_dir_all(&remote_base).unwrap();

        fs::create_dir_all(
            remote_base
                .join(OLD_SUB_AGENT_DATA_DIR)
                .join(INFRA_AGENT_ID),
        )
        .unwrap();

        let migration_pairs = get_migration_list(&local_base, &remote_base);
        assert_eq!(
            migration_pairs.len(),
            6,
            "Migration list should contain 6 items (4 ac + 2 remote infra)"
        );

        for (old_path, _) in migration_pairs.iter() {
            let parent = old_path.parent().unwrap();
            fs::create_dir_all(parent).unwrap();
            File::create(old_path).unwrap();
        }

        let result = move_and_rename(&local_base, &remote_base);
        assert!(result.is_ok());

        for (old_path, new_path) in migration_pairs.iter() {
            assert!(old_path.exists());
            assert!(new_path.exists());
        }
    }
}
