use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Deserializer};

use crate::agent_type::{
    definition::Variables, error::AgentTypeError,
    runtime_config::templateable_value::TemplateableValue, templates::Templateable,
};

/// Represents the file system configuration for the deployment of an agent.
///
/// It is a key-value structure in which every key is an identifier and the value is a file entry.
/// See [FileEntry] for details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem(HashMap<String, FileEntry>);

/// A file entry consists on a path and its content. The path must always be relative,
/// as these represent files that will be created for a sub-agent's scope (i.e. in AC's
/// auto-generated directory for that sub-agent).
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
struct FileEntry {
    path: PathBuf,
    content: TemplateableValue<String>,
}

impl<'de> Deserialize<'de> for FileSystem {
    fn deserialize<D>(deserializer: D) -> Result<FileSystem, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let map = HashMap::<_, FileEntry>::deserialize(deserializer)?;
        // Perform validations on the provided Paths
        if let Err(errs) = validate_file_entries(map.values()) {
            Err(Error::custom(errs.join(", ")))
        } else {
            Ok(FileSystem(map))
        }
    }
}

impl Templateable for FileSystem {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.0
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, _>>()
            .map(FileSystem)
    }
}

impl Templateable for FileEntry {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            path: self.path,
            content: self.content.template_with(variables)?,
        })
    }
}

fn validate_file_entries<'a>(
    entries: impl Iterator<Item = &'a FileEntry>,
) -> Result<(), Vec<String>> {
    // All elements are unique in the Path
    let mut seen_paths = HashSet::new();
    let mut errors = Vec::new();

    entries.map(|entry| &entry.path).for_each(|p| {
        // Inserting already-inserted items in the hashset evaluates to `false`.
        if !seen_paths.insert(p) {
            let p = p.display();
            errors.push(format!("All paths must be unique. Found duplicate: {p}"));
        }
        // Absolute paths are not permitted
        else if !p.is_relative() {
            let p = p.display();
            errors.push(format!("All paths must be relative. Found absolute: {p}"));
        }
        // Directories must not escape the base directory
        if let Err(e) = escapes_basedir(p) {
            errors.push(e);
        }
    });

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn escapes_basedir(path: &Path) -> Result<(), String> {
    path.components()
        .try_fold(0, |depth, comp| match comp {
            Component::Normal(_) => Ok(depth + 1),
            Component::ParentDir if depth > 0 => Ok(depth - 1),
            Component::ParentDir => Err(format!("{} escapes the base directory", path.display())),
            Component::CurDir => Ok(depth),
            // Disallow other non-supported variants like roots or prefixes
            Component::RootDir | Component::Prefix(_) => {
                Err(format!("{} has an invalid path component", path.display()))
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::can_basic_path("valid/path", Result::is_ok)]
    #[case::can_nested_dirs("another/valid/path", Result::is_ok)]
    #[case::can_back_one_level("basedir/somedir/../valid/path", Result::is_ok)]
    #[case::can_change_basedir("basedir/dir/../dir/../../newbasedir/path", Result::is_ok)]
    #[case::no_absolute("/absolute/path", Result::is_err)]
    #[case::no_escapes_basedir("..//invalid/path", Result::is_err)]
    #[case::no_complex_escapes_basedir("basedir/dir/../dir/../../../outdir/path", Result::is_err)]
    fn validate_basedir_safety(
        #[case] path: &str,
        #[case] validation: impl Fn(&Result<(), String>) -> bool,
    ) {
        let path = Path::new(path);
        assert!(validation(&escapes_basedir(path)));
    }
}
