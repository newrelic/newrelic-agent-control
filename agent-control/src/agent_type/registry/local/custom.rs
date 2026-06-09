use crate::agent_type::definition::AgentTypeDefinition;
use std::{fs, path::PathBuf};
use tracing::{debug, error};

/// Reads and returns the custom (dynamic) agent type definitions found in the given directory.
///
/// These are the agent types a user drops in the dynamic agent types directory to override or
/// extend the embedded ones for PoCs and testing. If there is an error reading the directory or
/// deserializing one of its files, the error is logged and the offending entry skipped.
pub(super) fn custom_definitions(path: PathBuf) -> Vec<AgentTypeDefinition> {
    let Ok(dir_entries) = fs::read_dir(path.clone()).inspect_err(|err| {
        debug!(
            path = path.display().to_string(),
            "Could not read Custom agent types directory: {err}"
        )
    }) else {
        return vec![];
    };

    let mut entries: Vec<_> = dir_entries.flatten().collect();
    // The order of entries returned by the `dir_entries` iterator is platform and filesystem
    // dependent. To ensure a consistent order of processing, we sort the entries by their path.
    // This is important because the current implementation uses a HashMap, and inserting
    // already existing keys will overwrite the former values.
    entries.sort_by_key(|a| a.path());

    entries
        .into_iter()
        .flat_map(|entry| {
            let file = entry.path();
            fs::read(file.clone())
                .inspect_err(|e| debug!(error = %e, "Skipping file: {file:?}"))
                .ok()
                .and_then(|content| {
                    debug!("Loading Dynamic Agent Type: {file:?}");
                    serde_saphyr::from_slice::<AgentTypeDefinition>(content.as_slice())
                        .inspect_err(
                            |e| error!(error = %e, "Could not parse Dynamic Agent Type: {file:?}"),
                        )
                        .ok()
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    /// Minimal valid agent type definition with the given name.
    fn agent_type_yaml(name: &str) -> String {
        format!(
            r#"
namespace: ns
name: {name}
version: 0.0.0
platform: kubernetes
variables: {{}}
deployment:
  objects: {{}}
"#
        )
    }

    fn write_file(dir: &Path, name: &str, content: &str) {
        File::create(dir.join(name))
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    #[test]
    fn returns_empty_for_nonexistent_directory() {
        assert!(custom_definitions(PathBuf::from("/nonexistent/path")).is_empty());
    }

    #[test]
    fn parses_valid_files_skipping_invalid_and_empty_ones() {
        let tmp_dir = tempdir().expect("failed to create local temp dir");
        write_file(tmp_dir.path(), "valid", &agent_type_yaml("valid"));
        write_file(tmp_dir.path(), "invalid", "not an agent type");
        write_file(tmp_dir.path(), "empty", "");

        let definitions = custom_definitions(tmp_dir.path().to_path_buf());

        assert_eq!(definitions.len(), 1);
        assert_eq!(
            definitions[0].agent_type_id(),
            &AgentTypeID::try_from("ns/valid:0.0.0").unwrap()
        );
    }

    #[test]
    fn returns_definitions_in_sorted_file_order() {
        let tmp_dir = tempdir().expect("failed to create local temp dir");
        // File order on disk is platform dependent, so the loader sorts by path to be
        // deterministic. This ordering is what lets the registry resolve precedence on collisions.
        write_file(tmp_dir.path(), "02_second", &agent_type_yaml("second"));
        write_file(tmp_dir.path(), "01_first", &agent_type_yaml("first"));

        let definitions = custom_definitions(tmp_dir.path().to_path_buf());

        let ids: Vec<_> = definitions
            .iter()
            .map(|d| d.agent_type_id().name())
            .collect();
        assert_eq!(ids, vec!["first", "second"]);
    }
}
