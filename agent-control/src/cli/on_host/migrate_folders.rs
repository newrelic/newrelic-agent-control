use crate::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA, INSTANCE_ID_FILENAME,
    STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG,
};
use crate::cli::error::CliError;
use crate::on_host::file_store::build_config_name;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error};

const LOCAL_DATA_DIR: &str = "/etc/newrelic-agent-control";
const REMOTE_DATA_DIR: &str = "/var/lib/newrelic-agent-control";
const OLD_ENV_FILE_NAME: &str = "newrelic-agent-control.conf";
const NEW_ENV_FILE_NAME: &str = "systemd-env.conf";
const VALUES_FOLDER: &str = "values";
// old folder and file names
const OLD_CONFIG_AGENT_CONTROL_FILE_NAME: &str = "config.yaml";
const OLD_IDENTIFIERS_YAML: &str = "identifiers.yaml";
const OLD_CONFIG_SUB_AGENT_FILE_NAME: &str = "values.yaml";
#[cfg(target_family = "unix")]
const OLD_SUB_AGENT_DATA_DIR: &str = "fleet/agents.d";
#[cfg(target_family = "windows")]
const OLD_SUB_AGENT_DATA_DIR: &str = "fleet\\agents.d";

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
    migration_pairs.push((
        local_base.join(OLD_CONFIG_AGENT_CONTROL_FILE_NAME),
        local_base
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
    ));
    migration_pairs.push((
        local_base.join(OLD_ENV_FILE_NAME),
        local_base.join(NEW_ENV_FILE_NAME),
    ));

    // --- REMOTE ---
    migration_pairs.push((
        remote_base.join(OLD_CONFIG_AGENT_CONTROL_FILE_NAME),
        remote_base
            .join(FOLDER_NAME_FLEET_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG)),
    ));
    migration_pairs.push((
        remote_base.join(OLD_IDENTIFIERS_YAML),
        remote_base
            .join(FOLDER_NAME_FLEET_DATA)
            .join(AGENT_CONTROL_ID)
            .join(INSTANCE_ID_FILENAME),
    ));
}
fn discover_and_add_sub_agents(
    migration_pairs: &mut Vec<(PathBuf, PathBuf)>,
    old_agents_dir: &Path,
    new_base_dir: &Path,
    new_data_folder: &str,
    config_key: &str,
    is_remote: bool,
) {
    if !old_agents_dir.is_dir() {
        return;
    }
    let entries = if let Ok(entries) = fs::read_dir(old_agents_dir) {
        entries
    } else {
        debug!(
            "Could not read old agent directory '{}'",
            old_agents_dir.display(),
        );
        return;
    };

    let agent_iter = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|p| p.file_name().map(|f| f.to_owned()).map(|f| (p, f)));

    for (old_agent_dir, agent_id) in agent_iter {
        debug!(
            "Discovered old agent '{}', adding to migration.",
            old_agent_dir.display()
        );

        let new_agent_dir = new_base_dir.join(new_data_folder).join(agent_id);

        migration_pairs.push((
            old_agent_dir
                .join(VALUES_FOLDER)
                .join(OLD_CONFIG_SUB_AGENT_FILE_NAME),
            new_agent_dir.join(build_config_name(config_key)),
        ));

        if is_remote {
            migration_pairs.push((
                old_agent_dir.join(OLD_IDENTIFIERS_YAML),
                new_agent_dir.join(INSTANCE_ID_FILENAME),
            ));
        }
    }
}

fn get_migration_list(local_base: &Path, remote_base: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut migration_pairs = Vec::new();

    add_agent_control_files(&mut migration_pairs, local_base, remote_base);

    discover_and_add_sub_agents(
        &mut migration_pairs,
        &local_base.join(OLD_SUB_AGENT_DATA_DIR),
        local_base,
        FOLDER_NAME_LOCAL_DATA,
        STORE_KEY_LOCAL_DATA_CONFIG,
        false,
    );

    discover_and_add_sub_agents(
        &mut migration_pairs,
        &remote_base.join(OLD_SUB_AGENT_DATA_DIR),
        remote_base,
        FOLDER_NAME_FLEET_DATA,
        STORE_KEY_OPAMP_DATA_CONFIG,
        true,
    );

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
    fn test_full_migration_logic_with_dynamic_agents() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();

        let local_base = root.join("etc");
        let remote_base = root.join("var");
        fs::create_dir_all(&local_base).unwrap();
        fs::create_dir_all(&remote_base).unwrap();

        let agent1_id = "nr-infra";
        let agent2_id = "nrdot";
        let agent3_id = "my-custom-agent";

        fs::create_dir_all(local_base.join(OLD_SUB_AGENT_DATA_DIR).join(agent1_id)).unwrap();
        fs::create_dir_all(remote_base.join(OLD_SUB_AGENT_DATA_DIR).join(agent1_id)).unwrap();
        fs::create_dir_all(local_base.join(OLD_SUB_AGENT_DATA_DIR).join(agent2_id)).unwrap();
        fs::create_dir_all(remote_base.join(OLD_SUB_AGENT_DATA_DIR).join(agent3_id)).unwrap();

        let migration_pairs = get_migration_list(&local_base, &remote_base);
        assert_eq!(
            migration_pairs.len(),
            10,
            "Migration list should contain all dynamically found items"
        );

        let has_custom_agent = migration_pairs
            .iter()
            .any(|(_old, new)| new.to_str().unwrap_or_default().contains(agent3_id));
        assert!(
            has_custom_agent,
            "Dynamically discovered agent 'my-custom-agent' was not found in migration pairs"
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

        let migration_pairs_2 = get_migration_list(&local_base, &remote_base);
        assert_eq!(migration_pairs_2.len(), 10);

        let result_2 = migrate();
        assert!(result_2.is_ok());
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
}
