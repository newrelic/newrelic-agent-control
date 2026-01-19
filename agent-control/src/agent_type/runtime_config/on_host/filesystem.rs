//! Module defining the file system configuration for sub-agents.
//!
//! This includes files and directories that should be created for the sub-agent at runtime,
//! based on templated content and paths. The paths are always relative to the sub-agent's
//! dedicated directory created by agent-control
//! (usually something like `/var/lib/newrelic-agent-control/filesystem/<SUB_AGENT_ID>`).
//! The files are created in a dedicated `files/` subdirectory, while directories are created in
//! a dedicated `directories/` subdirectory, to avoid name clashes.

use std::{
    collections::HashMap,
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
use serde::Deserialize;

pub mod rendered;

/// Represents the file system configuration for the deployment of a host agent. Consisting of
/// a set of directories (map keys) which in turn contain a set of files (nested map keys) with
/// their respective content (map values).
///
/// It would be equivalent to a YAML mapping of this format:
/// ```yaml
/// filesystem:
///   "path/to/my-dir":
///      # YAML content, expected to be a mapping string -> yaml
///      filepath1: "file1 content"
///      filepath2: | # multi-line string content
///        key: value
///   # fully templated content, expected to render to a valid YAML mapping string -> string
///   "another/path/to/my-dir": ${nr-var:some_var_that_renders_to_a_yaml_mapping}
/// ```
///
/// The files can be hardcoded, with the contents possibly containing templates, or the whole set of
/// files can be templated so a directory contains an arbitrary number of files (a place to use a
/// `map[string]yaml` variable type). **The paths cannot be templated.**
///
/// See [`AgentDirectoryEntry`] and [`DirEntriesType`] for more details.
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct FileSystem(HashMap<SafePath, DirEntriesType>);

/// A path to a file or directory that has been validated to be "safe",
/// i.e. relative and not escaping its base directory (e.g. with parent dir specifiers like `..`).
#[derive(Debug, Default, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(try_from = "PathBuf")]
pub struct SafePath(PathBuf);

/// Allow borrowing the inner [`Path`] from a [`SafePath`].
impl AsRef<Path> for SafePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Try to create a [`SafePath`] from a [`PathBuf`], validating that the path is relative
/// and does not escape its base directory. If the path is invalid, an error string is returned
/// containing a comma-separated list of the issues found.
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

/// The type of items present in a directory entry.
///
/// There are two supported modes:
///   1. A fixed set of entries, where each entry's content can be templated.
///      This implies the number and names of the entries are known at parse time.
///   2. A fully templated set of entries, where it's expected that a full template is provided as
///      a placeholder for later rendering.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
enum DirEntriesType {
    /// A directory with a fixed set of entries (i.e. files). Each entry's content can be templated.
    /// E.g.
    /// ```yaml
    /// "my/dir":
    ///   filepath1: "file1 content with ${nr-var:some_var}"
    ///   filepath2: "file2 content"
    /// ```
    FixedWithTemplatedContent(HashMap<SafePath, TemplateableValue<String>>),

    /// A directory with a fully templated set of entries, where it's expected that a full template
    /// is provided that renders to a valid YAML mapping of a safe [`PathBuf`] to [`String`].
    /// E.g.
    /// ```yaml
    /// "my/templated/dir":
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
pub struct DirEntriesMap(HashMap<SafePath, String>);

impl Templateable for FileSystem {
    type Output = rendered::FileSystem;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        if let Some(TrivialValue::String(filesystem_dir)) = variables
            .get(
                &Namespace::SubAgent
                    .namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
            )
            .and_then(Variable::get_final_value)
        {
            let filesystem = self
                .0
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        // The only place where we construct a `SafePath` directly, prepending the
                        // sub-agent's filesystem directory to the user-provided relative path.
                        // FIXME: when we fix the templating and make the agent type definitions
                        // type-safe, we will make sure to always build correct "final paths" here.
                        SafePath(PathBuf::from(filesystem_dir.clone()).join(k)),
                        v.template_with(variables)?,
                    ))
                })
                .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;
            Ok(rendered::FileSystem(filesystem))
        } else {
            Err(AgentTypeError::MissingValue(
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
            ))
        }
    }
}

impl FileSystem {
    /// Returns the list of directory paths (keys) defined in this filesystem configuration.
    pub fn dir_paths(&self) -> impl Iterator<Item = &SafePath> {
        self.0.keys()
    }
}

impl Templateable for DirEntriesType {
    type Output = rendered::DirEntriesType;
    /// Replaces placeholders in the content with values from the `Variables` map.
    ///
    /// Behaves differently depending on the variant:
    /// - For `FixedWithTemplatedContent`, it templates each entry's content individually.
    /// - For `FullyTemplated`, it templates the entire content as a single unit, expecting it to
    ///   be a valid (YAML) mapping of safe `PathBuf` to `String`.
    ///
    /// See [`TemplateableValue<DirEntriesMap>::template_with`] for details.
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        match self {
            DirEntriesType::FixedWithTemplatedContent(map) => {
                let rendered_map = map
                    .into_iter()
                    .map(|(k, v)| Ok((k, v.template_with(variables)?)))
                    .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;
                Ok(rendered::DirEntriesType::FixedWithTemplatedContent(
                    rendered_map,
                ))
            }
            DirEntriesType::FullyTemplated(tv) => Ok(rendered::DirEntriesType::FullyTemplated(
                tv.template_with(variables)?,
            )),
        }
    }
}

impl Templateable for TemplateableValue<DirEntriesMap> {
    type Output = DirEntriesMap;
    /// Performs the templating of the defined directory entries for this sub-agent in the case where
    /// they were fully templated (see [`DirEntriesType::FullyTemplated`]).
    ///
    /// The paths present in the DirectoryEntry structures are always assumed to start from the
    /// sub-agent's dedicated directory.
    ///
    /// Besides, we know the paths are relative and don't go above their base dir (e.g. `/../..`)
    /// due to the parse-time validations of [`FileSystem`], so here we "safely" prepend the
    /// provided base dir to them, as it must be defined in the variables passed to the sub-agent.
    /// If the value of the sub-agent's dedicated directory is missing, the templating fails.
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        // Template content as a string first. Then parse as a YAML and attempt to convert to the
        // expected HashMap<PathBuf, String> type.
        let templated_string = self.template.template_with(variables)?;
        let value: HashMap<SafePath, String> = if templated_string.is_empty() {
            HashMap::new()
        } else {
            let map_string_value: HashMap<SafePath, serde_yaml::Value> =
                serde_yaml::from_str(&templated_string).map_err(|e| {
                    AgentTypeError::ValueNotParseableFromString(format!(
                        "Could not parse templated directory items as YAML: {e}"
                    ))
                })?;

            // Convert the serde_yaml::Value (i.e. the file contents) to String
            map_string_value
                .into_iter()
                .map(|(k, v)| Ok((k, output_string(v)?)))
                .collect::<Result<HashMap<_, _>, serde_yaml::Error>>()?
        };

        Ok(DirEntriesMap(value))
    }
}

/// Converts a serde_yaml::Value to a String.
/// If the value is already a String, it is returned as-is.
/// Otherwise, it is serialized to a YAML string using serde_yaml.
fn output_string(value: serde_yaml::Value) -> Result<String, serde_yaml::Error> {
    match value {
        // Pass the string directly (serde_yaml inserts literal syntax for multi-line strings)
        serde_yaml::Value::String(s) => Ok(s),
        // Else serialize the value to a YAML string using the default methods
        v => serde_yaml::to_string(&v),
    }
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
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
            Variable::new_final_string_variable("/base/dir"),
        )]);

        let filesystem_entry = FileSystem(HashMap::from([(
            PathBuf::from("my/path").try_into().unwrap(),
            DirEntriesType::FixedWithTemplatedContent(HashMap::from([(
                PathBuf::from("my/file/path").try_into().unwrap(),
                TemplateableValue::from_template("some content".to_string()),
            )])),
        )]));

        let rendered = filesystem_entry.template_with(&variables);
        assert!(rendered.is_ok());
        let rendered = rendered.unwrap();
        assert!(rendered.0.len() == 1);

        let expected_filesystem = rendered::FileSystem(HashMap::from([(
            SafePath(PathBuf::from("/base/dir/my/path")),
            rendered::DirEntriesType::FixedWithTemplatedContent(HashMap::from([(
                PathBuf::from("my/file/path").try_into().unwrap(),
                "some content".to_string(),
            )])),
        )]));

        assert_eq!(rendered, expected_filesystem);
    }

    #[test]
    fn invalid_filepath_rendering_nonexisting_subagent_basepath() {
        // If the sub-agent variable (nr-sub) containing the agent's filesystem dir is missing,
        // templating must fail.
        let variables = Variables::default();

        let filesystem_entry = FileSystem(HashMap::from([(
            PathBuf::from("my/path").try_into().unwrap(),
            DirEntriesType::FixedWithTemplatedContent(HashMap::from([(
                PathBuf::from("my/file/path").try_into().unwrap(),
                TemplateableValue::new("some content".to_string()),
            )])),
        )]));

        let rendered = filesystem_entry.template_with(&variables);
        assert!(rendered.is_err());
        let rendered_err = rendered.unwrap_err();
        assert!(matches!(rendered_err, AgentTypeError::MissingValue(_)));
        assert_eq!(
            rendered_err.to_string(),
            format!(
                "missing value for key: {}",
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR)
            )
        );
    }

    #[rstest]
    #[case::valid_filesystem_parse("basic/path", |r: Result<_, _>| r.is_ok())]
    #[case::windows_style_path(r"some\\windows\\style\\path", |r: Result<_, _>| r.is_ok())]
    #[case::invalid_absolute_path("/absolute/path", |r: Result<_, serde_yaml::Error>| r.is_err())]
    #[case::invalid_reaches_parentdir("basedir/dir/../dir/../../../outdir/path", |r: Result<_, serde_yaml::Error>| r.is_err())]
    // #[case::invalid_windows_path_prefix(r"C:\\absolute\\windows\\path", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // #[case::invalid_windows_root_device("C:", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // #[case::invalid_windows_server_path(r"\\\\server\\share", |r: Result<_, serde_yaml::Error>| r.is_err_and(|e| e.to_string().contains("invalid path component")))]
    // TODO add windows paths to check that this handles the `Component::Prefix(_)` case correctly
    fn file_entry_parsing(
        #[case] path: &str,
        #[case] validation: impl Fn(Result<DirEntriesType, serde_yaml::Error>) -> bool,
    ) {
        let yaml = format!("\"{path}\": \"some random content\"");
        let parsed = serde_yaml::from_str::<DirEntriesType>(&yaml);
        let parsed_display = format!("{parsed:?}");
        assert!(validation(parsed), "input: {yaml}, parsed:{parsed_display}");
    }

    const EXAMPLE_FILESYSTEM: &str = r#"
"path/to/my-dir":
    filepath1: "file1 content"
    filepath2: |
        key: ${nr-var:some_var}
"another/path/to/my-dir":
    ${nr-var:some_var_that_renders_to_a_yaml_mapping}
"#;

    #[test]
    fn parse_valid_directories() {
        let parsed: Result<FileSystem, _> = serde_yaml::from_str(EXAMPLE_FILESYSTEM);
        assert!(
            parsed.as_ref().is_ok_and(|p| p.0.len() == 2),
            "Parsed directories: {parsed:?}"
        );

        let parsed = parsed.unwrap().0;
        let my_dir = parsed
            .get(&SafePath(PathBuf::from("path/to/my-dir")))
            .unwrap();
        assert!(matches!(
            my_dir,
            DirEntriesType::FixedWithTemplatedContent(_)
        ));

        let another_dir = parsed
            .get(&SafePath(PathBuf::from("another/path/to/my-dir")))
            .unwrap();
        assert!(matches!(another_dir, DirEntriesType::FullyTemplated(_)));
    }

    const FILESYSTEM_EXAMPLE: &str = r#"
"some/files":
    "path/to/my-file": "something ${nr-var:some_file_var}"
    "another/path/to/my-file": |
        some
        multi-line
        content
"path/to/my-dir":
    filepath1: "file1 content"
    filepath2: |
        key: ${nr-var:some_dir_var}
"another/path/to/my-dir":
    ${nr-var:some_var_that_renders_to_a_yaml_mapping}
"#;

    #[test]
    fn parse_and_template_filesystem() {
        let parsed = serde_yaml::from_str::<FileSystem>(FILESYSTEM_EXAMPLE);
        assert!(
            parsed.as_ref().is_ok_and(|fs| fs.0.len() == 3),
            "Parsed filesystem: {parsed:?}"
        );

        let parsed = parsed.unwrap();
        let variables = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
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
    }

    #[test]
    fn rendered_files() {
        let parsed = serde_yaml::from_str::<FileSystem>(FILESYSTEM_EXAMPLE);
        assert!(
            parsed.as_ref().is_ok_and(|fs| fs.0.len() == 3),
            "Parsed filesystem: {parsed:?}"
        );

        let parsed = parsed.unwrap();
        let variables = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
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
                PathBuf::from("/test/base/dir/another/path/to/my-dir/fileA"),
                String::from("contentA"),
            ),
            (
                PathBuf::from("/test/base/dir/path/to/my-dir/filepath1"),
                String::from("file1 content"),
            ),
            (
                PathBuf::from("/test/base/dir/path/to/my-dir/filepath2"),
                String::from("key: dir_var_value\n"),
            ),
            (
                PathBuf::from("/test/base/dir/some/files/path/to/my-file"),
                String::from("something file_var_value"),
            ),
            (
                PathBuf::from("/test/base/dir/some/files/another/path/to/my-file"),
                String::from("some\nmulti-line\ncontent\n"),
            ),
            (
                PathBuf::from("/test/base/dir/another/path/to/my-dir/fileB"),
                String::from("multi-line\ncontentB"),
            ),
        ];
        let rendered = templated.expand_paths();
        assert_eq!(
            rendered.len(),
            expected_rendered.len(),
            "Rendered filesystem not same size as expected: {rendered:?}, expected: {expected_rendered:?}"
        );

        assert!(
            rendered.iter().all(|(r_p, r_s)| expected_rendered
                .iter()
                .any(|(e_p, e_s)| e_p == r_p && e_s == r_s)),
            "Rendered filesystem not matching expected: {rendered:?}, expected: {expected_rendered:?}"
        );
    }
}
