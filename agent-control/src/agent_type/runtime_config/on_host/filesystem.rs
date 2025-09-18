use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

use crate::agent_type::{
    agent_attributes::AgentAttributes,
    definition::Variables,
    error::AgentTypeError,
    runtime_config::templateable_value::TemplateableValue,
    templates::Templateable,
    trivial_value::TrivialValue,
    variable::{Variable, namespace::Namespace},
};
use serde::{Deserialize, Deserializer, de::Error};

pub const FILES_SUBDIR: &str = "files";
pub const DIRECTORIES_SUBDIR: &str = "directories";

/// Represents the file system configuration for the deployment of an agent.
///
/// It would be equivalent to a YAML mapping of this format:
/// ```yaml
/// filesystem:
///   files:
///     my-file: # an ID of sorts for the file, might be used in the future for var references
///       relative_path: path/to/my-file
///       content: "something" # String content
///   directories:
///     my-dir: # an ID of sorts for the directory, might be used in the future for var references
///       relative_path: path/to/my-dir
///       items: # YAML content, expected to be a mapping string -> yaml
///         filepath1: "file1 content"
///         filepath2: | # multi-line string content
///           key: value
///     another-dir:
///       relative_path: another/path/to/my-dir
///       items: | # fully templated content, expected to render to a valid YAML mapping string -> string
///         ${nr-var:some_var_that_renders_to_a_yaml_mapping}
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem {
    files: HashMap<String, AgentFileEntry>,
    directories: HashMap<String, AgentDirectoryEntry>,
}

/// A file entry consists on a path and its content. The path must always be relative,
/// as these represent files that will be created for a sub-agent's scope (i.e. in AC's
/// auto-generated directory for that sub-agent).
#[derive(Debug, Default, Clone, PartialEq)]
struct AgentFileEntry {
    relative_path: PathBuf,
    content: TemplateableValue<String>,
}

/// A directory entry consists on a path and its items. The path must always be relative,
/// as these represent directories that will be created for a sub-agent's scope (i.e. in AC's
/// auto-generated directory for that sub-agent).
#[derive(Debug, Default, Clone, PartialEq)]
struct AgentDirectoryEntry {
    relative_path: PathBuf,
    items: DirEntriesType,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
enum DirEntriesType {
    /// A directory with a fixed set of entries, but each entry's content can be templated.
    /// E.g.
    /// ```yaml
    /// items:
    ///  filepath1: "file1 content with ${nr-var:some_var}"
    ///  filepath2: "file2 content"
    /// ```
    FixedWithTemplatedContent(HashMap<PathBuf, TemplateableValue<String>>),

    /// A directory with a fully templated set of entries, where it's expected that a full template
    /// is provided that renders to a valid YAML mapping of `PathBuf` to `String`.
    /// E.g.
    /// ```yaml
    /// items: |
    ///   ${nr-var:some_var_that_renders_to_a_yaml_mapping}
    /// ```
    FullyTemplated(TemplateableValue<DirEntriesMap>),
}

impl Default for DirEntriesType {
    fn default() -> Self {
        DirEntriesType::FixedWithTemplatedContent(HashMap::default())
    }
}

/// A helper newtype to allow implementing `Templateable` for `TemplateableValue<HashMap<PathBuf, String>>`
/// without running into orphan rule issues.
#[derive(Debug, Default, PartialEq, Clone)]
struct DirEntriesMap(HashMap<PathBuf, String>);

impl FileSystem {
    /// Returns the internal file entries as a [`HashMap<PathBuf, String>`].
    ///
    /// **WARNING**: This must be called **after** the rendering process has finished or else AC will crash!
    pub fn rendered(self) -> HashMap<PathBuf, String> {
        todo!();
        // self.0
        //     .into_values()
        //     .map(|v| (v.relative_path, v.content.get()))
        //     .collect()
    }
}

impl<'de> Deserialize<'de> for FileSystem {
    fn deserialize<D>(deserializer: D) -> Result<FileSystem, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PreValidationFileSystem {
            #[serde(default)]
            files: HashMap<String, AgentFileEntry>,
            #[serde(default)]
            directories: HashMap<String, AgentDirectoryEntry>,
        }
        let PreValidationFileSystem { files, directories } =
            PreValidationFileSystem::deserialize(deserializer)?;

        // Validate that all paths are unique across files and directories
        let file_paths = files.values().map(|e| &e.relative_path);
        let dir_paths = directories.values().map(|e| &e.relative_path);
        if let Err(e) = validate_unique_paths(file_paths.chain(dir_paths)) {
            return Err(Error::custom(format!(
                "duplicate file paths are not allowed. Found duplicate path: '{}'",
                e.display()
            )));
        }

        Ok(Self { files, directories })
    }
}

impl<'de> Deserialize<'de> for AgentFileEntry {
    fn deserialize<D>(deserializer: D) -> Result<AgentFileEntry, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PreValidationFileEntry {
            relative_path: PathBuf,
            content: TemplateableValue<String>,
        }

        let PreValidationFileEntry {
            relative_path,
            content,
        } = PreValidationFileEntry::deserialize(deserializer)?;

        // Perform validations on the provided Paths
        if let Err(errs) = validate_file_entry_path(&relative_path) {
            Err(Error::custom(errs.join(", ")))
        } else {
            Ok(Self {
                relative_path,
                content,
            })
        }
    }
}

impl<'de> Deserialize<'de> for AgentDirectoryEntry {
    fn deserialize<D>(deserializer: D) -> Result<AgentDirectoryEntry, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PreValidationDirectoryEntry {
            relative_path: PathBuf,
            items: DirEntriesType,
        }

        let PreValidationDirectoryEntry {
            relative_path,
            items,
        } = PreValidationDirectoryEntry::deserialize(deserializer)?;
        // Perform validations on the provided Paths
        if let Err(errs) = validate_file_entry_path(&relative_path) {
            Err(Error::custom(errs.join(", ")))
        } else {
            Ok(Self {
                relative_path,
                items,
            })
        }
    }
}

impl Templateable for FileSystem {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let files = self
            .files
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;

        let directories = self
            .directories
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;
        Ok(Self { files, directories })
    }
}

impl Templateable for AgentFileEntry {
    /// Performs the templating of the defined file entries for this sub-agent.
    ///
    /// The paths present in the FileEntry structures are always assumed to start from the
    /// sub-agent's dedicated directory **and** a dedicated directory for stand-alone files
    /// ([`FILES_SUBDIR`]).
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
                relative_path: PathBuf::from(generated_dir)
                    .join(FILES_SUBDIR)
                    .join(self.relative_path),
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

impl Templateable for DirEntriesType {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        match self {
            DirEntriesType::FixedWithTemplatedContent(map) => {
                let rendered_map = map
                    .into_iter()
                    .map(|(k, v)| Ok((k, v.template_with(variables)?)))
                    .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;
                Ok(DirEntriesType::FixedWithTemplatedContent(rendered_map))
            }
            DirEntriesType::FullyTemplated(tv) => {
                Ok(DirEntriesType::FullyTemplated(tv.template_with(variables)?))
            }
        }
    }
}

impl Templateable for AgentDirectoryEntry {
    /// Performs the templating of the defined directory entries for this sub-agent.
    ///
    /// The paths present in the DirectoryEntry structures are always assumed to start from the
    /// sub-agent's dedicated directory **and** a dedicated directory for stand-alone directories
    /// ([`DIRECTORIES_SUBDIR`]).
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
            let rendered_directory_entry = Self {
                relative_path: PathBuf::from(generated_dir)
                    .join(DIRECTORIES_SUBDIR)
                    .join(self.relative_path),
                items: self.items.template_with(variables)?,
            };
            Ok(rendered_directory_entry)
        } else {
            Err(AgentTypeError::MissingValue(
                Namespace::SubAgent.namespaced_name(AgentAttributes::GENERATED_DIR),
            ))
        }
    }
}

impl Templateable for TemplateableValue<DirEntriesMap> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        // Template content as a string first. Then parse as a YAML and attempt to convert to the
        // expected HashMap<PathBuf, String> type.
        let templated_string = self.template.clone().template_with(variables)?;
        let value: HashMap<PathBuf, String> = if templated_string.is_empty() {
            HashMap::new()
        } else {
            serde_yaml::from_str(&templated_string).map_err(|e| {
                AgentTypeError::ValueNotParseableFromString(format!(
                    "Could not parse templated directory items as YAML mapping: {e}"
                ))
            })?
        };
        Ok(Self {
            template: self.template,
            value: Some(DirEntriesMap(value)),
        })
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
/// E.g. via relative path specifiers like `./../../some_path`.
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

        let file_entry = AgentFileEntry {
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

        let file_entry = AgentFileEntry {
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
        #[case] validation: impl Fn(Result<AgentFileEntry, serde_yaml::Error>) -> bool,
    ) {
        let yaml = format!(
            "relative_path: \"{}\"\ncontent: \"some random content\"",
            path
        );
        let parsed = serde_yaml::from_str::<AgentFileEntry>(&yaml);
        let parsed_display = format!("{parsed:?}");
        assert!(validation(parsed), "{}", parsed_display);
    }

    const EXAMPLE_FILES: &str = r#"
my-file:
    relative_path: path/to/my-file
    content: "something"
another-file:
    relative_path: another/path/to/my-file
    content: |
        some
        multi-line
        content
"#;

    const EXAMPLE_DIRS: &str = r#"
my-dir:
    relative_path: path/to/my-dir
    items:
        filepath1: "file1 content"
        filepath2: |
            key: ${nr-var:some_var}
another-dir:
    relative_path: another/path/to/my-dir
    items: |
      ${nr-var:some_var_that_renders_to_a_yaml_mapping}
"#;

    #[test]
    fn parse_valid_files() {
        let parsed: Result<HashMap<String, AgentFileEntry>, _> =
            serde_yaml::from_str(EXAMPLE_FILES);
        assert!(
            parsed.as_ref().is_ok_and(|p| p.len() == 2),
            "Parsed filesystem: {parsed:?}"
        );

        let parsed = parsed.unwrap();

        let my_file = parsed.get("my-file").unwrap();
        assert_eq!(my_file.relative_path, PathBuf::from("path/to/my-file"));

        let another_file = parsed.get("another-file").unwrap();
        assert_eq!(
            another_file.relative_path,
            PathBuf::from("another/path/to/my-file")
        );
    }

    #[test]
    fn parse_valid_directories() {
        let parsed: Result<HashMap<String, AgentDirectoryEntry>, _> =
            serde_yaml::from_str(EXAMPLE_DIRS);
        assert!(
            parsed.as_ref().is_ok_and(|p| p.len() == 2),
            "Parsed directories: {parsed:?}"
        );

        let parsed = parsed.unwrap();
        let my_dir = parsed.get("my-dir").unwrap();
        assert_eq!(my_dir.relative_path, PathBuf::from("path/to/my-dir"));
        assert!(matches!(
            my_dir.items,
            DirEntriesType::FixedWithTemplatedContent(_)
        ));

        let another_dir = parsed.get("another-dir").unwrap();
        assert_eq!(
            another_dir.relative_path,
            PathBuf::from("another/path/to/my-dir")
        );
        assert!(matches!(
            another_dir.items,
            DirEntriesType::FullyTemplated(_)
        ));
    }
}
