use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Deserializer};

use crate::agent_type::{
    agent_attributes::AgentAttributes,
    definition::Variables,
    error::AgentTypeError,
    runtime_config::templateable_value::TemplateableValue,
    templates::Templateable,
    trivial_value::TrivialValue,
    variable::{Variable, namespace::Namespace},
};

/// Represents the file system configuration for the deployment of an agent.
///
/// It would be equivalent to a YAML mapping of this format:
/// ```yaml
/// filesystem:
///   files:
///     my-file:
///       relative_path: path/to/my-file
///       content: "something" # String content
///   directories:
///     my-dir:
///       relative_path: path/to/my-dir
///       items: # YAML content, expected to be a mapping string -> yaml
///         filepath1: "file1 content"
///         filepath2:
///           key: value
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem(HashMap<String, FileEntry>);

impl FileSystem {
    /// Returns the internal file entries as a [`HashMap<PathBuf, String>`].
    ///
    /// **WARNING**: This must be called **after** the rendering process has finished or else AC will crash!
    pub fn rendered(self) -> HashMap<PathBuf, String> {
        self.0
            .into_values()
            .map(|v| (v.relative_path, v.content.get()))
            .collect()
    }
}

impl<'de> Deserialize<'de> for FileSystem {
    fn deserialize<D>(deserializer: D) -> Result<FileSystem, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = HashMap::<String, FileEntry>::deserialize(deserializer)?;
        if let Err(e) = validate_unique_paths(entries.values().map(|e| &e.relative_path)) {
            return Err(serde::de::Error::custom(format!(
                "duplicate file paths are not allowed. Found duplicate path: '{}'",
                e.display()
            )));
        }
        Ok(Self(entries))
    }
}

/// A file entry consists on a path and its content. The path must always be relative,
/// as these represent files that will be created for a sub-agent's scope (i.e. in AC's
/// auto-generated directory for that sub-agent).
#[derive(Debug, Default, Clone, PartialEq)]
struct FileEntry {
    relative_path: PathBuf,
    content: TemplateableValue<String>,
}

impl<'de> Deserialize<'de> for FileEntry {
    fn deserialize<D>(deserializer: D) -> Result<FileEntry, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        #[derive(Deserialize)]
        struct PreValidationFileEntry {
            relative_path: PathBuf,
            content: TemplateableValue<String>,
        }

        let entry = PreValidationFileEntry::deserialize(deserializer)?;
        // Perform validations on the provided Paths
        if let Err(errs) = validate_file_entry_path(&entry.relative_path) {
            Err(Error::custom(errs.join(", ")))
        } else {
            Ok(Self {
                relative_path: entry.relative_path,
                content: entry.content,
            })
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
    /// Performs the templating of the defined file entries for this sub-agent.
    ///
    /// The paths present in the FileEntry structures are always assumed to start from the
    /// sub-agent's dedicated directory.
    ///
    /// Besides, we know the paths are relative and don't go above their base dir (e.g. `/../..`)
    /// due to the parse-time validations of [`FileSystem`], so here we "safely" prepend the
    /// provided base dir to them, as it must be defined in the variables passed to the sub-agent.
    /// If the value of the sub-agent's dedicated directory is missing, the templating fails.
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        if let Some(TrivialValue::String(generated_dir)) = variables
            .get(&Namespace::SubAgent.namespaced_name(AgentAttributes::GENERATED_DIR))
            .and_then(Variable::get_final_value)
        {
            let rendered_file_entry = Self {
                relative_path: PathBuf::from(generated_dir).join(self.relative_path),
                content: self.content.template_with(variables)?,
            };
            Ok(rendered_file_entry)
        } else {
            Err(AgentTypeError::MissingValue(
                Namespace::SubAgent.namespaced_name(AgentAttributes::GENERATED_DIR),
            ))
        }
    }
}

fn validate_unique_paths<'a>(
    mut paths: impl Iterator<Item = &'a PathBuf>,
) -> Result<(), &'a PathBuf> {
    let mut seen_paths = HashSet::new();
    // Inserting already-inserted items in the hashset evaluates to `false`.
    paths.try_for_each(|p| if seen_paths.insert(p) { Ok(()) } else { Err(p) })
}

fn validate_file_entry_path(path: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    if !path.is_relative() {
        let p = path.display();
        errors.push(format!(
            "Only relative paths are allowed. Found absolute: {p}"
        ));
    }
    // Paths must not escape the base directory
    if let Err(e) = check_basedir_escape_safety(path) {
        errors.push(e);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Makes sure the passed directory goes not traverse outside the directory where it's contained.
/// E.g. via relative path specificers like `./../../some_path`.
///
/// Returns an error string if this property does not hold.
fn check_basedir_escape_safety(path: &Path) -> Result<(), String> {
    path.components().try_for_each(|comp| match comp {
        Component::Normal(_) | Component::CurDir => Ok(()),
        // Disallow other non-supported variants like roots or prefixes
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => Err(format!(
            "path '{}' has an invalid component: '{}'",
            path.display(),
            comp.as_os_str().to_string_lossy()
        )),
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::can_basic_path("valid/path", Result::is_ok)]
    #[case::can_nested_dirs("another/valid/path", Result::is_ok)]
    #[case::can_use_curdir("basedir/somedir/./valid/path", Result::is_ok)]
    #[case::no_use_parentdir("basedir/somedir/../valid/path", Result::is_err)]
    #[case::no_change_basedir("basedir/dir/../dir/../../newbasedir/path", Result::is_err)]
    #[case::no_absolute("/absolute/path", Result::is_err)]
    #[case::no_escapes_basedir("..//invalid/path", Result::is_err)]
    #[case::no_complex_escapes_basedir("basedir/dir/../dir/../../../outdir/path", Result::is_err)]
    fn validate_basedir_safety(
        #[case] path: &str,
        #[case] validation: impl Fn(&Result<(), String>) -> bool,
    ) {
        let path = Path::new(path);
        assert!(validation(&check_basedir_escape_safety(path)));
    }

    #[test]
    fn valid_filepath_rendering() {
        let variables = Variables::from_iter(vec![(
            Namespace::SubAgent.namespaced_name(AgentAttributes::GENERATED_DIR),
            Variable::new_final_string_variable("/base/dir"),
        )]);

        let file_entry = FileEntry {
            relative_path: PathBuf::from("my/file/path"),
            content: TemplateableValue::new("some content".to_string()),
        };

        let rendered = file_entry.template_with(&variables);
        assert!(rendered.is_ok());
        assert_eq!(
            rendered.unwrap().relative_path,
            PathBuf::from("/base/dir/my/file/path")
        );
    }

    #[test]
    fn invalid_filepath_rendering_nonexisting_subagent_basepath() {
        let variables = Variables::default();

        let file_entry = FileEntry {
            relative_path: PathBuf::from("my/file/path"),
            content: TemplateableValue::new("some content".to_string()),
        };

        let rendered = file_entry.template_with(&variables);
        assert!(rendered.is_err());
        let rendered_err = rendered.unwrap_err();
        assert!(matches!(rendered_err, AgentTypeError::MissingValue(_)));
        assert_eq!(
            rendered_err.to_string(),
            format!(
                "missing value for key: `{}`",
                Namespace::SubAgent.namespaced_name(AgentAttributes::GENERATED_DIR)
            )
        );
    }

    #[rstest]
    #[case::valid_filesystem_parse("basic/path", |r: Result<_, _>| r.is_ok())]
    #[case::windows_style_path(r"some\\windows\\style\\path", |r: Result<_, _>| r.is_ok())]
    #[case::invalid_absolute_path("/absolute/path", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("absolute: /absolute/path")))]
    #[case::invalid_reaches_parentdir("basedir/dir/../dir/../../../outdir/path", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid component: '..'")))]
    // #[case::invalid_windows_path_prefix(r"C:\\absolute\\windows\\path", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // #[case::invalid_windows_root_device("C:", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // #[case::invalid_windows_server_path(r"\\\\server\\share", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // TODO add windows paths to check that this handles the `Component::Prefix(_)` case correctly
    fn file_entry_parsing(
        #[case] path: &str,
        #[case] validation: impl Fn(Result<FileEntry, serde_yaml::Error>) -> bool,
    ) {
        let yaml = format!("path: \"{}\"\ncontent: \"some random content\"", path);
        let parsed = serde_yaml::from_str::<FileEntry>(&yaml);
        let parsed_display = format!("{parsed:?}");
        assert!(validation(parsed), "{}", parsed_display);
    }
}
