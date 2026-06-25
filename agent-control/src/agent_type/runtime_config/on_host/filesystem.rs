//! Module defining the file system configuration for sub-agents.
//!
//! Every entry under `filesystem:` is declared with an explicit `kind:` (`file`, `dir`, or
//! `dir_content_from_map`). Directory trees are built recursively via the `entries:` field on
//! `kind: dir`. A directory's contents may also be projected from a `map[string]yaml` variable
//! using `kind: dir_content_from_map`, where map keys become filenames and values become file
//! bodies.
//!
//! Top-level keys are interpreted relative to the sub-agent's dedicated filesystem directory
//! (`${nr-sub:filesystem_agent_dir}`).

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
use serde::de::Error;

pub mod rendered;

/// Filesystem configuration for an on-host sub-agent: a tree of files, directories, and
/// directories whose contents are projected from `map[string]yaml` variables.
///
/// Every entry is tagged with a `kind:`. `dir` entries may contain further entries under
/// `entries:`, recursively.
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct FileSystem(HashMap<SafePath, FilesystemEntry>);

/// One entry in a filesystem tree. The `kind` discriminator selects which fields are required.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilesystemEntry {
    /// A single file with literal or templated content.
    File { text: TemplateableValue<String> },
    /// An explicitly declared directory. Children, if any, live under `entries:`.
    Dir {
        #[serde(default)]
        entries: HashMap<SafePath, FilesystemEntry>,
    },
    /// A directory whose set of files is computed at deploy time from a `map[string]yaml`
    /// variable. Map keys become filenames; values become file contents.
    DirContentFromMap {
        source: TemplateableValue<DirEntriesMap>,
    },
}

/// A path validated to be relative and not escaping its base directory (no `..`, no absolute
/// roots, no Windows prefixes).
#[derive(Debug, Default, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(try_from = "PathBuf")]
pub struct SafePath(PathBuf);

impl AsRef<Path> for SafePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

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

/// Helper carrying the rendered output of a `${nr-var:map[string]yaml}` source — exists
/// to satisfy the orphan rule when implementing `Templateable` for `TemplateableValue<_>`.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DirEntriesMap(HashMap<SafePath, String>);

impl Templateable for FileSystem {
    type Output = rendered::FileSystem;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let base_dir = filesystem_agent_dir(variables)?;

        let entries = self
            .0
            .into_iter()
            .map(|(key, entry)| {
                // The only place we construct a final-on-disk path: prepend the sub-agent's
                // dedicated filesystem dir to the user-provided relative top-level key.
                let path = PathBuf::from(&base_dir).join(&key);
                Ok((path, entry.template_with(variables)?))
            })
            .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;

        Ok(rendered::FileSystem(entries))
    }
}

impl Templateable for FilesystemEntry {
    type Output = rendered::RenderedEntry;

    /// Recursively templates this entry into a [`rendered::RenderedEntry`] tree. Sub-paths in the
    /// resulting tree are kept relative to their parent; the absolute prefix is applied once at
    /// the top level by [`FileSystem::template_with`].
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        match self {
            FilesystemEntry::File { text } => Ok(rendered::RenderedEntry::File(
                text.template_with(variables)?,
            )),
            FilesystemEntry::Dir { entries } => {
                let children = entries
                    .into_iter()
                    .map(|(k, v)| Ok((PathBuf::from(k), v.template_with(variables)?)))
                    .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;
                Ok(rendered::RenderedEntry::Dir(children))
            }
            FilesystemEntry::DirContentFromMap { source } => {
                let map = source.template_with(variables)?;
                let files = map
                    .0
                    .into_iter()
                    .map(|(k, content)| (PathBuf::from(k), content))
                    .collect();
                Ok(rendered::RenderedEntry::DirContentFromMap(files))
            }
        }
    }
}

fn filesystem_agent_dir(variables: &Variables) -> Result<String, AgentTypeError> {
    let key = Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR);
    match variables.get(&key).and_then(Variable::get_final_value) {
        Some(TrivialValue::String(s)) => Ok(s.clone()),
        _ => Err(AgentTypeError::MissingValue(key)),
    }
}

impl Templateable for TemplateableValue<DirEntriesMap> {
    type Output = DirEntriesMap;

    /// Templates the source string of a `dir_content_from_map` entry, then parses the result as a
    /// YAML mapping `filename -> contents`. Empty templated string yields an empty map.
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let templated_string = self.template.template_with(variables)?;
        let value: HashMap<SafePath, String> = if templated_string.is_empty() {
            HashMap::new()
        } else {
            let map_string_value: HashMap<SafePath, serde_json::Value> =
                serde_saphyr::from_str(&templated_string).map_err(|e| {
                    AgentTypeError::ValueNotParseableFromString(format!(
                        "Could not parse templated directory items as YAML: {e}"
                    ))
                })?;

            map_string_value
                .into_iter()
                .map(|(k, v)| Ok((k, output_string(v)?)))
                .collect::<Result<HashMap<_, _>, serde_saphyr::Error>>()?
        };

        Ok(DirEntriesMap(value))
    }
}

/// Converts a serde_json::Value to a String. Strings pass through; other variants are serialized
/// as YAML.
fn output_string(value: serde_json::Value) -> Result<String, serde_saphyr::Error> {
    match value {
        // Pass the string directly (serde_saphyr inserts literal syntax for multi-line strings)
        serde_json::Value::String(s) => Ok(s),
        // Else serialize the value to a YAML string using the default methods
        v => serde_saphyr::to_string(&v).map_err(|e| serde_saphyr::Error::custom(e.to_string())),
    }
}

/// Validates that a file entry path is a single, relative, non-escaping leaf segment.
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
    // Each key must be a single leaf segment, not a sub-path.
    if let Err(e) = check_single_segment(path) {
        errors.push(e);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(", "))
    }
}

/// A key must be exactly one `Normal` path segment (a leaf). This rejects multi-segment keys
/// (e.g. `newrelic-infra/newrelic-integrations/logging` — declare nested trees explicitly with
/// `kind: dir` + `entries:`) and also non-canonical single-segment spellings such as `./config`.
/// Escaping components (`..`, root, Windows prefixes) are handled by `check_basedir_escape_safety`.
fn check_single_segment(path: &Path) -> Result<(), String> {
    let mut components = path.components();
    if let (Some(Component::Normal(_)), None) = (components.next(), components.next()) {
        return Ok(());
    }
    Err(format!(
        "path `{}` must be a single path segment (a leaf); declare nested directories \
         explicitly with `kind: dir` and `entries:`",
        path.display()
    ))
}

/// Rejects paths that traverse outside their base directory (e.g. `./../../some_path`) so that
/// no sub-agent can write outside its dedicated dir.
fn check_basedir_escape_safety(path: &Path) -> Result<(), String> {
    path.components().try_for_each(|comp| match comp {
        Component::Normal(_) | Component::CurDir => Ok(()),
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => Err(format!(
            "path `{}` has an invalid component: `{}`",
            path.display(),
            comp.as_os_str().to_string_lossy()
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::runtime_config::on_host::filesystem::rendered::RenderedEntry;
    use fs::directory_manager::DirectoryManagerFs;
    use fs::file::LocalFile;
    use rstest::rstest;
    use serde_json::Value;
    use tempfile::TempDir;

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
    fn templates_top_level_file() {
        let variables = Variables::from_iter(vec![(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
            Variable::new_final_string_variable("/base/dir"),
        )]);

        let fs_input = FileSystem(HashMap::from([(
            PathBuf::from("newrelic.yaml").try_into().unwrap(),
            FilesystemEntry::File {
                text: TemplateableValue::from_template("hello".to_string()),
            },
        )]));

        let rendered = fs_input.template_with(&variables).unwrap();

        let expected = rendered::FileSystem(HashMap::from([(
            PathBuf::from("/base/dir/newrelic.yaml"),
            RenderedEntry::File("hello".to_string()),
        )]));
        assert_eq!(rendered, expected);
    }

    #[test]
    fn templating_fails_without_filesystem_agent_dir_variable() {
        let variables = Variables::default();
        let fs_input = FileSystem(HashMap::from([(
            PathBuf::from("any").try_into().unwrap(),
            FilesystemEntry::Dir {
                entries: HashMap::new(),
            },
        )]));

        let err = fs_input.template_with(&variables).unwrap_err();
        assert!(matches!(err, AgentTypeError::MissingValue(_)));
        assert_eq!(
            err.to_string(),
            format!(
                "missing value for key: {}",
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR)
            )
        );
    }

    #[rstest]
    #[case::single_segment("config", true)]
    // `./config` is a non-canonical spelling of `config` (distinct map key, same on-disk path).
    #[case::leading_curdir("./config", false)]
    // Multi-segment keys are rejected: nested dirs must be declared with `kind: dir` + `entries:`.
    #[case::multi_segment("agent/data", false)]
    #[case::dot_segment("agent/./data", false)]
    #[case::absolute("/etc", false)]
    #[case::dotdot("agent/../escape", false)]
    fn safe_path_parsing(#[case] path: &str, #[case] should_parse: bool) {
        let yaml = format!(
            r#"
"{path}":
  kind: dir
"#
        );
        let parsed = serde_saphyr::from_str::<FileSystem>(&yaml);
        assert_eq!(
            parsed.is_ok(),
            should_parse,
            "input: {yaml}, parsed: {parsed:?}"
        );
    }

    #[cfg(windows)]
    #[rstest]
    #[case::drive_with_path(r"C:\\absolute\\windows\\path")]
    #[case::drive_root("C:")]
    #[case::unc_server_share(r"\\\\server\\share")]
    fn safe_path_parsing_rejects_windows_prefixes(#[case] path: &str) {
        let yaml = format!(
            r#"
"{path}":
  kind: dir
"#
        );
        let parsed = serde_saphyr::from_str::<FileSystem>(&yaml);
        assert!(parsed.is_err(), "input: {yaml}, parsed: {parsed:?}");
    }

    const FILESYSTEM_EXAMPLE: &str = r#"
newrelic-infra.yaml:
  kind: file
  text: ${nr-var:config_agent}

config:
  kind: dir

logging.d:
  kind: dir_content_from_map
  source: ${nr-var:config_logging}

agent:
  kind: dir
  entries:
    data:
      kind: dir
    integrations.d:
      kind: dir_content_from_map
      source: ${nr-var:config_integrations}
    newrelic-infra.yaml:
      kind: file
      text: ${nr-var:config_agent}
"#;

    fn example_variables(base_dir: &str) -> Variables {
        Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(base_dir),
            ),
            (
                Namespace::Variable.namespaced_name("config_agent"),
                Variable::new_final_string_variable("license_key: REDACTED\n"),
            ),
            (
                Namespace::Variable.namespaced_name("config_integrations"),
                Variable::new(
                    String::default(),
                    false,
                    None,
                    Some(HashMap::from([
                        (
                            "nri-mysql.yaml".to_string(),
                            Value::String("integration: mysql".to_string()),
                        ),
                        (
                            "nri-redis.yaml".to_string(),
                            Value::String("integration: redis".to_string()),
                        ),
                    ])),
                ),
            ),
            (
                Namespace::Variable.namespaced_name("config_logging"),
                Variable::new(
                    String::default(),
                    false,
                    None,
                    Some(HashMap::from([(
                        "syslog.yaml".to_string(),
                        Value::String("logs: []".to_string()),
                    )])),
                ),
            ),
        ])
    }

    #[test]
    fn parses_all_three_kinds() {
        let parsed = serde_saphyr::from_str::<FileSystem>(FILESYSTEM_EXAMPLE).unwrap();
        assert_eq!(parsed.0.len(), 4);

        let file_entry = parsed
            .0
            .get(&SafePath(PathBuf::from("newrelic-infra.yaml")))
            .unwrap();
        assert!(matches!(file_entry, FilesystemEntry::File { .. }));

        let empty_dir = parsed.0.get(&SafePath(PathBuf::from("config"))).unwrap();
        assert!(matches!(empty_dir, FilesystemEntry::Dir { entries } if entries.is_empty()));

        let dir_from_map = parsed.0.get(&SafePath(PathBuf::from("logging.d"))).unwrap();
        assert!(matches!(
            dir_from_map,
            FilesystemEntry::DirContentFromMap { .. }
        ));

        let nested_dir = parsed.0.get(&SafePath(PathBuf::from("agent"))).unwrap();
        let FilesystemEntry::Dir { entries } = nested_dir else {
            panic!("expected agent to be a Dir, got {nested_dir:?}");
        };
        assert_eq!(entries.len(), 3);
        assert!(matches!(
            entries.get(&SafePath(PathBuf::from("data"))).unwrap(),
            FilesystemEntry::Dir { .. }
        ));
        assert!(matches!(
            entries
                .get(&SafePath(PathBuf::from("integrations.d")))
                .unwrap(),
            FilesystemEntry::DirContentFromMap { .. }
        ));
        assert!(matches!(
            entries
                .get(&SafePath(PathBuf::from("newrelic-infra.yaml")))
                .unwrap(),
            FilesystemEntry::File { .. }
        ));
    }

    #[test]
    fn rejects_unknown_kind() {
        let yaml = r#"
foo:
  kind: invented
"#;
        let parsed = serde_saphyr::from_str::<FileSystem>(yaml);
        assert!(parsed.is_err(), "parsed: {parsed:?}");
    }

    /// Templating + writing the example to disk produces every expected file with the right
    /// content, an empty directory for `kind: dir` with no entries, and `dir_content_from_map`
    /// projects the map's keys as files.
    #[test]
    fn rendered_files_on_disk() {
        let parsed = serde_saphyr::from_str::<FileSystem>(FILESYSTEM_EXAMPLE).unwrap();
        let tmp_dir = TempDir::new().unwrap();
        let variables = example_variables(&tmp_dir.path().to_string_lossy());

        let templated = parsed.template_with(&variables).unwrap();
        templated.write(&LocalFile, &DirectoryManagerFs).unwrap();

        let expected_files = [
            (
                tmp_dir.path().join("newrelic-infra.yaml"),
                "license_key: REDACTED\n",
            ),
            (
                tmp_dir.path().join("agent/newrelic-infra.yaml"),
                "license_key: REDACTED\n",
            ),
            (
                tmp_dir.path().join("agent/integrations.d/nri-mysql.yaml"),
                "integration: mysql",
            ),
            (
                tmp_dir.path().join("agent/integrations.d/nri-redis.yaml"),
                "integration: redis",
            ),
            (tmp_dir.path().join("logging.d/syslog.yaml"), "logs: []"),
        ];

        for (path, expected) in expected_files.iter() {
            let actual = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            assert_eq!(&actual, expected, "content mismatch at {}", path.display());
        }

        let empty_dir = tmp_dir.path().join("config");
        assert!(empty_dir.is_dir(), "empty dir not created at {empty_dir:?}");

        let nested_empty_dir = tmp_dir.path().join("agent/data");
        assert!(
            nested_empty_dir.is_dir(),
            "nested empty dir not created at {nested_empty_dir:?}"
        );
    }
}
