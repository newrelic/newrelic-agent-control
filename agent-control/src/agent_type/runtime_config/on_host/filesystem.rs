//! Module defining the file system configuration for sub-agents.
//!
//! This includes files and directories that should be created for the sub-agent at runtime,
//! based on templated content and paths. The paths are always relative to the sub-agent's
//! dedicated directory created by agent-control
//! (usually something like `/var/lib/newrelic-agent-control/auto-generated/<SUB_AGENT_ID>`).
//! The files are created in a dedicated `files/` subdirectory, while directories are created in
//! a dedicated `directories/` subdirectory, to avoid name clashes.

use std::{
    collections::{HashMap, HashSet},
    io::{Error as IOError, ErrorKind},
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
///       path: path/to/my-file
///       content: "something" # String content
///   directories:
///     my-dir: # an ID of sorts for the directory, might be used in the future for var references
///       path: path/to/my-dir
///       items: # YAML content, expected to be a mapping string -> yaml
///         filepath1: "file1 content"
///         filepath2: | # multi-line string content
///           key: value
///     another-dir:
///       path: another/path/to/my-dir
///       items: | # fully templated content, expected to render to a valid YAML mapping string -> string
///         ${nr-var:some_var_that_renders_to_a_yaml_mapping}
/// ```
///
/// For now, the identifiers for files and directories (e.g. `my-file` and `my-dir` in the example
/// above) are not used for anything, so the same identifiers can be used on either side (files,
/// directories), but they might be used in the future to reference these entries
/// from variables or other parts of the configuration, in which case duplicates might stop being
/// allowed.
///
/// The `path` fields, on the other hand, are allowed to be equal between files and
/// directories, but not within each section (i.e. two files cannot have the same `path`,
/// and neither can two directories). This is validated at parse time and after templating.
///
/// Templating is only supported for file contents, not for IDs, nor file/directory names,
/// with the exception of directory items which might accept an arbitrary number of files
/// to place in a directory via templates (a place to use a `map[string]yaml` variable type).
/// See [`AgentDirectoryEntry`] and [`DirEntriesType`] for more details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem {
    files: HashMap<String, AgentFileEntry>,
    directories: HashMap<String, AgentDirectoryEntry>,
}

impl FileSystem {
    /// Returns the internal file entries as a [`HashMap<PathBuf, String>`] so they can
    /// be written into the actual host filesystem.
    ///
    /// **WARNING**: This must be called **after** the rendering process has finished
    /// or else AC might crash!
    pub fn rendered(self) -> HashMap<PathBuf, String> {
        // Retrieve files
        let files = self
            .files
            .into_values()
            .map(|v| (v.path.into(), v.content.get()));
        // Retrieve directories
        // A more elaborate operation, since each directory contains a collection of files inside
        // and we need to retrieve all of them, flattening into a single iterator to append to the
        // files above.
        let dirs = self.directories.into_values().flat_map(|d| d.rendered());
        files.chain(dirs).collect()
    }
}

/// Custom deserialization that validates all paths are unique inside both
/// files and directories definitions. Duplicates across files and directories are allowed.
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
        let file_paths = files.values().map(|e| &e.path);
        let dir_paths = directories.values().map(|e| &e.path);

        let mut errs = Vec::default();
        if let Err(e) = validate_unique_paths(file_paths) {
            errs.push(format!(
                "duplicate file paths are not allowed. Found duplicate path: '{}'",
                e.as_ref().display()
            ));
        }
        if let Err(e) = validate_unique_paths(dir_paths) {
            errs.push(format!(
                "duplicate directory paths are not allowed. Found duplicate path: '{}'",
                e.as_ref().display()
            ));
        }

        // Error out if there were any errors
        if errs.is_empty() {
            Ok(Self { files, directories })
        } else {
            Err(Error::custom(errs.join(", ")))
        }
    }
}

/// A path to a file or directory that has been validated to be "safe",
/// i.e. relative and not escaping its base directory (e.g. with parent dir specifiers like `..`).
#[derive(Debug, Default, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(try_from = "PathBuf")]
struct SafePath(PathBuf);

/// Allow borrowing the inner [`Path`] from a [`SafePath`].
impl AsRef<Path> for SafePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Try to create a [`SafePath`] from a [`PathBuf`], validating that the path is relative
/// and does not escape its base directory. If the path is invalid, an error string is returned
/// containing
impl TryFrom<PathBuf> for SafePath {
    type Error = IOError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        validate_file_entry_path(&value)
            .map_err(|e| IOError::new(ErrorKind::InvalidFilename, e))?;
        Ok(SafePath(value))
    }
}

impl From<SafePath> for PathBuf {
    fn from(value: SafePath) -> Self {
        value.0
    }
}

/// A file entry consists on a path and its content. The path must always be relative,
/// as these represent files that will be created for a sub-agent's scope (i.e. in AC's
/// auto-generated directory for that sub-agent).
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
struct AgentFileEntry {
    path: SafePath,
    content: TemplateableValue<String>,
}

/// A directory entry consists on a path and its items. The path must always be relative,
/// as these represent directories that will be created for a sub-agent's scope (i.e. in AC's
/// auto-generated directory for that sub-agent).
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
struct AgentDirectoryEntry {
    path: SafePath,
    items: DirEntriesType,
}

impl AgentDirectoryEntry {
    /// Returns the internal directory entries as a [`HashMap<PathBuf, String>`] so they can
    /// be written into the actual host filesystem.
    ///
    /// **WARNING**: This must be called **after** the rendering process has finished
    /// or else AC might crash!
    fn rendered(self) -> HashMap<PathBuf, String> {
        self.items.rendered_with(self.path)
    }
}

/// The type of items present in a directory entry.
///
/// There are two supported modes:
///   1. A fixed set of entries, where each entry's content can be templated. This implies the number
///      and names of the entries are known at parse time.
///   2. A fully templated set of entries, where it's expected that a full template is provided as
///      a placeholder for the full [`AgentDirectoryEntry.items`] field.
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
    FixedWithTemplatedContent(HashMap<SafePath, TemplateableValue<String>>),

    /// A directory with a fully templated set of entries, where it's expected that a full template
    /// is provided that renders to a valid YAML mapping of a safe [`PathBuf`] to [`String`].
    /// E.g.
    /// ```yaml
    /// items:
    ///   ${nr-var:some_var_that_renders_to_a_yaml_mapping}
    /// ```
    FullyTemplated(TemplateableValue<DirEntriesMap>),
}

impl Default for DirEntriesType {
    fn default() -> Self {
        DirEntriesType::FixedWithTemplatedContent(HashMap::default())
    }
}

impl DirEntriesType {
    /// Renders the directory entries as an iterator of [`HashMap<PathBuf, String>`] so they can
    /// be written into the actual host filesystem.
    ///
    /// **WARNING**: This must be called **after** the rendering process has finished
    /// or else AC might crash!
    fn rendered_with(self, path: impl AsRef<Path>) -> HashMap<PathBuf, String> {
        match self {
            DirEntriesType::FixedWithTemplatedContent(map) => map
                .into_iter()
                .map(|(k, v)| (path.as_ref().join(k), v.get()))
                .collect(),
            DirEntriesType::FullyTemplated(tv) => {
                let map = HashMap::from(tv.get());
                map.into_iter()
                    .map(|(k, v)| (path.as_ref().join(k), v))
                    .collect()
            }
        }
    }
}

/// A helper newtype to allow implementing `Templateable` for `TemplateableValue<HashMap<PathBuf, String>>`
/// without running into orphan rule issues.
#[derive(Debug, Default, PartialEq, Clone)]
struct DirEntriesMap(HashMap<SafePath, String>);

impl From<DirEntriesMap> for HashMap<SafePath, String> {
    fn from(value: DirEntriesMap) -> Self {
        value.0
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
                path: SafePath(
                    // The only place where we construct a `SafePath` directly, prepending the
                    // sub-agent's auto-generated directory and the `files/` subdir to the
                    // user-provided relative path.
                    // FIXME: when we fix the templating and make the agent type definitions
                    // type-safe, we will make sure to always construct a proper "final path" here.
                    PathBuf::from(generated_dir)
                        .join(FILES_SUBDIR)
                        .join(self.path),
                ),
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
                path: SafePath(
                    // The only place where we construct a `SafePath` directly, prepending the
                    // sub-agent's auto-generated directory and the `directories/` subdir to the
                    // user-provided relative path.
                    // FIXME: when we fix the templating and make the agent type definitions
                    // type-safe, we will make sure to always construct a proper "final path" here.
                    PathBuf::from(generated_dir)
                        .join(DIRECTORIES_SUBDIR)
                        .join(self.path),
                ),
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

impl Templateable for DirEntriesType {
    /// Replaces placeholders in the content with values from the `Variables` map.
    ///
    /// Behaves differently depending on the variant:
    /// - For `FixedWithTemplatedContent`, it templates each entry's content individually.
    /// - For `FullyTemplated`, it templates the entire content as a single unit, expecting it to
    ///   be a valid (YAML) mapping of safe `PathBuf` to `String`.
    ///
    /// See [`TemplateableValue<DirEntriesMap>::template_with`] for details.
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

impl Templateable for TemplateableValue<DirEntriesMap> {
    /// Performs the templating of the defined directory entries for this sub-agent in the case where
    /// they were fully templated (see [`DirEntriesType::FullyTemplated`]).
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
        // Template content as a string first. Then parse as a YAML and attempt to convert to the
        // expected HashMap<PathBuf, String> type.
        let templated_string = self.template.clone().template_with(variables)?;
        let value: HashMap<SafePath, String> = if templated_string.is_empty() {
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

/// Validates that all paths in the provided iterator are unique.
/// If a duplicate is found, returns an error with the first duplicate found.
fn validate_unique_paths<'a>(
    mut paths: impl Iterator<Item = &'a SafePath>,
) -> Result<(), &'a SafePath> {
    let mut seen_paths = HashSet::new();
    // Inserting already-inserted items in the hashset evaluates to `false`.
    paths.try_for_each(|p| if seen_paths.insert(p) { Ok(()) } else { Err(p) })
}

/// Validates that a file entry path is relative and does not escape its base directory.
/// Returns a comma-separated list of error messages, if any.
fn validate_file_entry_path(path: &Path) -> Result<(), String> {
    let mut errors = Vec::new();

    if !path.is_relative() {
        let p = path.display();
        errors.push(format!("path `{p}` is not relative"));
    }
    // Paths must not escape the base directory
    if let Err(e) = check_basedir_escape_safety(path) {
        errors.push(e);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(", "))
    }
}

/// Makes sure the passed directory goes not traverse outside the directory where it's contained.
/// E.g. via relative path specifiers like `./../../some_path`.
///
/// This would make files and directories "safe" to be created inside a sub-agent's dedicated
/// directory, as they would not be able to write outside of it
/// (tampering with other sub-agents or worse).
/// Returns an error string if this property does not hold.
fn check_basedir_escape_safety(path: &Path) -> Result<(), String> {
    path.components().try_for_each(|comp| match comp {
        Component::Normal(_) | Component::CurDir => Ok(()),
        // Disallow other non-supported variants like roots or prefixes
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => Err(format!(
            "path `{}` has an invalid component: `{}`",
            path.display(),
            comp.as_os_str().to_string_lossy()
        )),
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_yaml::Value;

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
            path: PathBuf::from("my/file/path").try_into().unwrap(),
            content: TemplateableValue::new("some content".to_string()),
        };

        let rendered = file_entry.template_with(&variables);
        assert!(rendered.is_ok());
        assert_eq!(
            rendered.unwrap().path.as_ref(),
            Path::new("/base/dir/files/my/file/path")
        );
    }

    #[test]
    fn invalid_filepath_rendering_nonexisting_subagent_basepath() {
        // If the sub-agent variable (nr-sub) containing the agent's auto-generated dir is missing,
        // templating must fail.
        let variables = Variables::default();

        let file_entry = AgentFileEntry {
            path: PathBuf::from("my/file/path").try_into().unwrap(),
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
    #[case::invalid_absolute_path("/absolute/path", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("`/absolute/path` is not relative")))]
    #[case::invalid_reaches_parentdir("basedir/dir/../dir/../../../outdir/path", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid component: `..`")))]
    // #[case::invalid_windows_path_prefix(r"C:\\absolute\\windows\\path", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // #[case::invalid_windows_root_device("C:", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // #[case::invalid_windows_server_path(r"\\\\server\\share", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // TODO add windows paths to check that this handles the `Component::Prefix(_)` case correctly
    fn file_entry_parsing(
        #[case] path: &str,
        #[case] validation: impl Fn(Result<AgentFileEntry, serde_yaml::Error>) -> bool,
    ) {
        let yaml = format!("path: \"{path}\"\ncontent: \"some random content\"");
        let parsed = serde_yaml::from_str::<AgentFileEntry>(&yaml);
        let parsed_display = format!("{parsed:?}");
        assert!(validation(parsed), "{parsed_display}");
    }

    const EXAMPLE_FILES: &str = r#"
my-file:
    path: path/to/my-file
    content: "something"
another-file:
    path: another/path/to/my-file
    content: |
        some
        multi-line
        content
"#;

    const EXAMPLE_DIRS: &str = r#"
my-dir:
    path: path/to/my-dir
    items:
        filepath1: "file1 content"
        filepath2: |
            key: ${nr-var:some_var}
another-dir:
    path: another/path/to/my-dir
    items:
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
        assert_eq!(my_file.path.as_ref(), Path::new("path/to/my-file"));

        let another_file = parsed.get("another-file").unwrap();
        assert_eq!(
            another_file.path.as_ref(),
            Path::new("another/path/to/my-file")
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
        assert_eq!(my_dir.path.as_ref(), Path::new("path/to/my-dir"));
        assert!(matches!(
            my_dir.items,
            DirEntriesType::FixedWithTemplatedContent(_)
        ));

        let another_dir = parsed.get("another-dir").unwrap();
        assert_eq!(
            another_dir.path.as_ref(),
            Path::new("another/path/to/my-dir")
        );
        assert!(matches!(
            another_dir.items,
            DirEntriesType::FullyTemplated(_)
        ));
    }

    const FILESYSTEM_EXAMPLE: &str = r#"
files:
  my-file:
      path: path/to/my-file
      content: "something ${nr-var:some_file_var}"
  another-file:
      path: another/path/to/my-file
      content: |
          some
          multi-line
          content
directories:
  my-dir:
      path: path/to/my-dir
      items:
          filepath1: "file1 content"
          filepath2: |
              key: ${nr-var:some_dir_var}
  another-dir:
      path: another/path/to/my-dir
      items:
        ${nr-var:some_var_that_renders_to_a_yaml_mapping}
"#;

    #[test]
    fn parse_and_template_filesystem() {
        let parsed = serde_yaml::from_str::<FileSystem>(FILESYSTEM_EXAMPLE);
        assert!(
            parsed
                .as_ref()
                .is_ok_and(|fs| fs.files.len() == 2 && fs.directories.len() == 2),
            "Parsed filesystem: {parsed:?}"
        );

        let parsed = parsed.unwrap();
        let variables = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::GENERATED_DIR),
                Variable::new_final_string_variable("/test/base/dir"),
            ),
            (
                Namespace::Variable.namespaced_name("some_file_var"),
                Variable::new_final_string_variable("file_var_value"),
            ),
            (
                Namespace::Variable.namespaced_name("some_dir_var"),
                Variable::new_final_string_variable("dir_var_value"),
            ),
            (
                Namespace::Variable.namespaced_name("some_var_that_renders_to_a_yaml_mapping"),
                // a map[string]yaml
                Variable::new(
                    String::default(),
                    false,
                    None,
                    Some(HashMap::from([
                        ("fileA".to_string(), Value::String("contentA".to_string())),
                        (
                            "fileB".to_string(),
                            Value::String("multi-line\ncontentB".to_string()),
                        ),
                    ])),
                ),
            ),
        ]);

        let templated = parsed.template_with(&variables);
        assert!(templated.is_ok(), "Templated filesystem: {templated:?}");
        // let templated = templated.unwrap();
    }

    #[test]
    fn rendered_files() {
        let parsed = serde_yaml::from_str::<FileSystem>(FILESYSTEM_EXAMPLE);
        assert!(
            parsed
                .as_ref()
                .is_ok_and(|fs| fs.files.len() == 2 && fs.directories.len() == 2),
            "Parsed filesystem: {parsed:?}"
        );

        let parsed = parsed.unwrap();
        let variables = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::GENERATED_DIR),
                Variable::new_final_string_variable("/test/base/dir"),
            ),
            (
                Namespace::Variable.namespaced_name("some_file_var"),
                Variable::new_final_string_variable("file_var_value"),
            ),
            (
                Namespace::Variable.namespaced_name("some_dir_var"),
                Variable::new_final_string_variable("dir_var_value"),
            ),
            (
                Namespace::Variable.namespaced_name("some_var_that_renders_to_a_yaml_mapping"),
                // a map[string]yaml
                Variable::new(
                    String::default(),
                    false,
                    None,
                    Some(HashMap::from([
                        ("fileA".to_string(), Value::String("contentA".to_string())),
                        (
                            "fileB".to_string(),
                            Value::String("multi-line\ncontentB".to_string()),
                        ),
                    ])),
                ),
            ),
        ]);

        let templated = parsed.template_with(&variables);
        assert!(templated.is_ok(), "Templated filesystem: {templated:?}");
        let templated = templated.unwrap();

        // Expected rendered paths with contents.
        // All paths must be prepended by the sub-agent's generated dir and the
        // corresponding `files/` or `directories/` subdir, depending on where they came from.
        // They also must have all variables rendered and have the correct content.
        let expected_rendered = [
            (
                PathBuf::from("/test/base/dir/directories/another/path/to/my-dir/fileA"),
                String::from("contentA"),
            ),
            (
                PathBuf::from("/test/base/dir/directories/path/to/my-dir/filepath1"),
                String::from("file1 content"),
            ),
            (
                PathBuf::from("/test/base/dir/directories/path/to/my-dir/filepath2"),
                String::from("key: dir_var_value\n"),
            ),
            (
                PathBuf::from("/test/base/dir/files/path/to/my-file"),
                String::from("something file_var_value"),
            ),
            (
                PathBuf::from("/test/base/dir/files/another/path/to/my-file"),
                String::from("some\nmulti-line\ncontent\n"),
            ),
            (
                PathBuf::from("/test/base/dir/directories/another/path/to/my-dir/fileB"),
                String::from("multi-line\ncontentB"),
            ),
        ];
        let rendered = templated.rendered();
        assert_eq!(
            rendered.len(),
            expected_rendered.len(),
            "Rendered filesystem not same size as expected: {rendered:?}, expected: {expected_rendered:?}"
        );

        assert!(
            rendered.iter().any(|(r_p, r_s)| expected_rendered
                .iter()
                .any(|(e_p, e_s)| e_p == r_p && e_s == r_s)),
            "Rendered filesystem not matching expected: {rendered:?}, expected: {expected_rendered:?}"
        );
    }
}
