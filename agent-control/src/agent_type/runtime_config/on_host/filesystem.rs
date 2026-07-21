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
    /// A single file whose bytes come from exactly one of `text` (literal or templated content)
    /// or `copy_from_file` (a source path copied byte-for-byte, e.g. a downloaded binary).
    File {
        /// The file's (possibly templated) content. Mutually exclusive with `copy_from_file`.
        #[serde(default)]
        text: Option<TemplateableValue<String>>,
        /// A (possibly templated) source path whose bytes are copied into this entry. Mutually
        /// exclusive with `text`.
        #[serde(default)]
        copy_from_file: Option<TemplateableValue<String>>,
    },
    /// An explicitly declared directory. Children, if any, live under `entries:`.
    Dir {
        /// The directory's child entries.
        #[serde(default)]
        entries: HashMap<SafePath, FilesystemEntry>,
    },
    /// A directory whose set of files is computed at deploy time from a `map[string]yaml`
    /// variable. Map keys become filenames; values become file contents.
    DirContentFromMap {
        /// The (templated) `map[string]yaml` source whose keys/values become files.
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

impl FileSystem {
    /// Whether no entries are declared.
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Collects all on-disk paths this filesystem declares, rooted under `base_dir`.
    pub fn declared_paths(&self, base_dir: &Path) -> DeclaredPaths {
        let mut declared = DeclaredPaths::default();
        for (key, entry) in &self.0 {
            collect_declared_paths(&base_dir.join(key), entry, &mut declared);
        }
        declared
    }

    /// Templates each entry and roots it under `base_dir`, prepending `base_dir` to each relative
    /// top-level key (the only place a final on-disk path is constructed).
    fn render_entries(
        self,
        base_dir: &Path,
        variables: &Variables,
    ) -> Result<HashMap<PathBuf, rendered::RenderedEntry>, AgentTypeError> {
        self.0
            .into_iter()
            .map(|(key, entry)| {
                let path = base_dir.join(&key);
                Ok((path, entry.template_with(variables)?))
            })
            .collect()
    }
}

impl Templateable for FileSystem {
    type Output = rendered::FileSystem;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let base_dir = PathBuf::from(filesystem_agent_dir(variables)?);
        let entries = self.render_entries(&base_dir, variables)?;
        Ok(rendered::FileSystem::new(entries))
    }
}

/// Files and directories shared across sub-agents, rooted at `${nr-sub:shared_filesystem_dir}`.
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct SharedFileSystem(FileSystem);

impl Templateable for SharedFileSystem {
    type Output = rendered::SharedFileSystem;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        // Return early if no shared entries are declared. Agent Types that don't use shared-filesystem aren't
        // enforced to provide the `${nr-sub:shared_filesystem_dir}` variable.
        if self.0.is_empty() {
            return Ok(rendered::SharedFileSystem::new(HashMap::new()));
        }
        let base_dir = PathBuf::from(shared_filesystem_dir(variables)?);
        let entries = self.0.render_entries(&base_dir, variables)?;
        Ok(rendered::SharedFileSystem::new(entries))
    }
}

/// The filesystem paths an Agent Type declares ownership of, rooted under a base directory and split by ownership
/// granularity.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeclaredPaths {
    /// Files owned individually. Several agents may drop sibling files into the same
    /// shared directory, but no two may own the exact same file path.
    pub owned_files: HashSet<PathBuf>,
    /// Directories owned as a whole: the entire subtree is managed as a unit, and no
    /// other agent may own anything at or under the path.
    pub owned_dirs: HashSet<PathBuf>,
    /// Shared `kind: dir` nodes. Several agents may declare the same one; a directory is only
    /// safe to remove once no agent declares it here anymore.
    pub dirs: HashSet<PathBuf>,
}

impl SharedFileSystem {
    /// Collects the on-disk paths this shared filesystem declares ownership of, rooted under
    /// `base_dir`.
    pub fn declared_paths(&self, base_dir: &Path) -> DeclaredPaths {
        let mut declared = DeclaredPaths::default();
        for (key, entry) in &self.0.0 {
            collect_declared_paths(&base_dir.join(key), entry, &mut declared);
        }
        declared
    }
}

/// Recursively gathers the paths declared by `entry` (rooted at `path`) into `declared`.
///
/// `Dir` entries are shared drop zones: the directory path is tracked separately in
/// `shared_dirs` (not `owned_files`/`owned_dirs`), since several agents may declare the same one.
fn collect_declared_paths(path: &Path, entry: &FilesystemEntry, declared: &mut DeclaredPaths) {
    match entry {
        FilesystemEntry::File { .. } => {
            declared.owned_files.insert(path.to_path_buf());
        }
        FilesystemEntry::Dir { entries, .. } => {
            declared.dirs.insert(path.to_path_buf());
            for (key, child) in entries {
                collect_declared_paths(&path.join(key), child, declared);
            }
        }
        FilesystemEntry::DirContentFromMap { .. } => {
            declared.owned_dirs.insert(path.to_path_buf());
        }
    }
}

impl Templateable for FilesystemEntry {
    type Output = rendered::RenderedEntry;

    /// Recursively templates this entry into a [`rendered::RenderedEntry`] tree. Sub-paths in the
    /// resulting tree are kept relative to their parent; the absolute prefix is applied once at
    /// the top level by [`FileSystem::template_with`].
    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        match self {
            FilesystemEntry::File {
                text,
                copy_from_file,
            } => {
                let content = match (text, copy_from_file) {
                    (Some(text), None) => {
                        rendered::FileContent::Text(text.template_with(variables)?)
                    }
                    (None, Some(copy_from_file)) => {
                        let source = PathBuf::from(copy_from_file.template_with(variables)?);
                        // `copy_from_file` may access to `${nr-sub:remote_dir}` (files in the agent filesystem)
                        // never arbitrary files from the host.
                        let base = copy_source_base(variables)?;
                        if !is_within_base(&source, &base) {
                            return Err(AgentTypeError::InvalidFileEntry(format!(
                                "`copy_from_file` source `{}` is outside the allowed base `{}`",
                                source.display(),
                                base.display()
                            )));
                        }
                        rendered::FileContent::Copy(source)
                    }
                    (Some(_), Some(_)) => {
                        return Err(AgentTypeError::InvalidFileEntry(
                            "cannot declare both `text` and `copy_from_file`".to_string(),
                        ));
                    }
                    (None, None) => {
                        return Err(AgentTypeError::InvalidFileEntry(
                            "must declare either `text` or `copy_from_file`".to_string(),
                        ));
                    }
                };
                Ok(rendered::RenderedEntry::File { content })
            }
            FilesystemEntry::Dir { entries } => {
                let children = entries
                    .into_iter()
                    .map(|(k, v)| Ok((PathBuf::from(k), v.template_with(variables)?)))
                    .collect::<Result<HashMap<_, _>, AgentTypeError>>()?;
                Ok(rendered::RenderedEntry::Dir { children })
            }
            FilesystemEntry::DirContentFromMap { source } => {
                let map = source.template_with(variables)?;
                let files = map
                    .0
                    .into_iter()
                    .map(|(k, content)| (PathBuf::from(k), content))
                    .collect();
                Ok(rendered::RenderedEntry::DirContentFromMap { files })
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

/// Resolves `${nr-sub:shared_filesystem_dir}`, the base for the shared filesystem tree.
fn shared_filesystem_dir(variables: &Variables) -> Result<String, AgentTypeError> {
    let key = Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_SHARED_FILESYSTEM_DIR);
    match variables.get(&key).and_then(Variable::get_final_value) {
        Some(TrivialValue::String(s)) => Ok(s.clone()),
        _ => Err(AgentTypeError::MissingValue(key)),
    }
}

/// The root a `copy_from_file` source must stay within: the sub-agent's AC data dir
/// (`${nr-sub:remote_dir}`), which contains packages and the per-agent and shared filesystem dirs.
fn copy_source_base(variables: &Variables) -> Result<PathBuf, AgentTypeError> {
    let key = Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_REMOTE_DIR);
    match variables.get(&key).and_then(Variable::get_final_value) {
        Some(TrivialValue::String(s)) => Ok(PathBuf::from(s)),
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

fn is_within_base(path: &Path, base_dir: &Path) -> bool {
    let has_escape = path.components().any(|c| matches!(c, Component::ParentDir));
    !has_escape && path.is_absolute() && path.starts_with(base_dir)
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
                text: Some(TemplateableValue::from_template("hello".to_string())),
                copy_from_file: None,
            },
        )]));

        let rendered = fs_input.template_with(&variables).unwrap();

        let expected = rendered::FileSystem::new(HashMap::from([(
            PathBuf::from("/base/dir/newrelic.yaml"),
            RenderedEntry::File {
                content: rendered::FileContent::Text("hello".to_string()),
            },
        )]));
        assert_eq!(rendered, expected);
    }

    #[test]
    fn parses_file_with_copy_from_file() {
        let yaml = r#"
nri-redis:
  kind: file
  copy_from_file: ${nr-sub:packages.nri-redis.dir}/nri-redis
"#;
        let parsed = serde_saphyr::from_str::<FileSystem>(yaml).unwrap();
        match parsed.0.get(&SafePath(PathBuf::from("nri-redis"))).unwrap() {
            FilesystemEntry::File {
                text,
                copy_from_file,
                ..
            } => {
                assert!(text.is_none(), "text must be absent");
                assert_eq!(
                    copy_from_file.as_ref().unwrap().template,
                    "${nr-sub:packages.nri-redis.dir}/nri-redis"
                );
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    /// Builds the reserved variables a filesystem render needs: the per-agent base dir and the
    /// remote dir that confines `copy_from_file` sources. Paths are passed through so callers can
    /// use real (absolute, platform-native) directories.
    fn fs_variables(agent_dir: &Path, remote_dir: &Path) -> Variables {
        Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(agent_dir.to_string_lossy()),
            ),
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_REMOTE_DIR),
                Variable::new_final_string_variable(remote_dir.to_string_lossy()),
            ),
        ])
    }

    #[test]
    fn copy_from_file_renders_to_copy_content() {
        // A real temp dir gives an absolute, platform-native base (`/data`-style literals are not
        // absolute on Windows, so they would fail the `is_within_base` confinement check there).
        let tmp_dir = TempDir::new().unwrap();
        let remote = tmp_dir.path();
        let agent_dir = remote.join("filesystem").join("agent");
        // A source within the remote dir (a package binary) is allowed.
        let source = remote.join("packages").join("nri-redis");

        let variables = fs_variables(&agent_dir, remote);
        let fs_input = FileSystem(HashMap::from([(
            PathBuf::from("nri-redis").try_into().unwrap(),
            FilesystemEntry::File {
                text: None,
                copy_from_file: Some(TemplateableValue::from_template(
                    source.to_string_lossy().to_string(),
                )),
            },
        )]));

        let rendered = fs_input.template_with(&variables).unwrap();

        let expected = rendered::FileSystem::new(HashMap::from([(
            agent_dir.join("nri-redis"),
            RenderedEntry::File {
                content: rendered::FileContent::Copy(source),
            },
        )]));
        assert_eq!(rendered, expected);
    }

    /// A `copy_from_file` source outside `${nr-sub:remote_dir}` (an absolute path elsewhere on the
    /// host, or a `..` traversal out of the base) is rejected at template time.
    #[test]
    fn copy_from_file_outside_base_is_rejected() {
        let tmp_dir = TempDir::new().unwrap();
        let remote = tmp_dir.path();
        let agent_dir = remote.join("filesystem").join("agent");

        // An absolute path outside the remote dir (a sibling of it), and a `..` escape out of it.
        // Both are built from the temp dir so they stay absolute and platform-native.
        let outside_sibling = remote
            .parent()
            .expect("temp dir has a parent")
            .join("outside-secret");
        let escapes_base = remote.join("..").join("outside-secret");

        for source in [outside_sibling, escapes_base] {
            let fs_input = FileSystem(HashMap::from([(
                PathBuf::from("nri-redis").try_into().unwrap(),
                FilesystemEntry::File {
                    text: None,
                    copy_from_file: Some(TemplateableValue::from_template(
                        source.to_string_lossy().to_string(),
                    )),
                },
            )]));

            let err = fs_input
                .template_with(&fs_variables(&agent_dir, remote))
                .unwrap_err();
            assert!(
                matches!(err, AgentTypeError::InvalidFileEntry(_)),
                "source {} should be rejected, got {err:?}",
                source.display()
            );
        }
    }

    #[rstest]
    #[case::both(
        Some(TemplateableValue::from_template("hi".to_string())),
        Some(TemplateableValue::from_template("/src".to_string()))
    )]
    #[case::neither(None, None)]
    fn file_entry_requires_exactly_one_source(
        #[case] text: Option<TemplateableValue<String>>,
        #[case] copy_from_file: Option<TemplateableValue<String>>,
    ) {
        let variables = Variables::from_iter(vec![(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
            Variable::new_final_string_variable("/base/dir"),
        )]);
        let fs_input = FileSystem(HashMap::from([(
            PathBuf::from("f").try_into().unwrap(),
            FilesystemEntry::File {
                text,
                copy_from_file,
            },
        )]));

        let err = fs_input.template_with(&variables).unwrap_err();
        assert!(
            matches!(err, AgentTypeError::InvalidFileEntry(_)),
            "expected InvalidFileEntry, got {err:?}"
        );
    }

    #[test]
    fn copy_from_file_copies_source_on_write() {
        let tmp_dir = TempDir::new().unwrap();
        let base = tmp_dir.path().join("base");
        let source = tmp_dir.path().join("nri-redis-src");
        let source_bytes = [0xFFu8, 0x00, b'b', b'i', b'n'];
        std::fs::write(&source, source_bytes).unwrap();

        // Source lives directly under the (temp) remote dir, so it passes confinement.
        let variables = fs_variables(&base, tmp_dir.path());
        let fs_input = FileSystem(HashMap::from([(
            PathBuf::from("nri-redis").try_into().unwrap(),
            FilesystemEntry::File {
                text: None,
                copy_from_file: Some(TemplateableValue::from_template(
                    source.to_string_lossy().to_string(),
                )),
            },
        )]));

        fs_input
            .template_with(&variables)
            .unwrap()
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        let dst = base.join("nri-redis");
        assert_eq!(
            std::fs::read(&dst).unwrap(),
            source_bytes,
            "destination must be a byte-for-byte copy of the source"
        );
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
        assert!(matches!(empty_dir, FilesystemEntry::Dir { entries, .. } if entries.is_empty()));

        let dir_from_map = parsed.0.get(&SafePath(PathBuf::from("logging.d"))).unwrap();
        assert!(matches!(
            dir_from_map,
            FilesystemEntry::DirContentFromMap { .. }
        ));

        let nested_dir = parsed.0.get(&SafePath(PathBuf::from("agent"))).unwrap();
        let FilesystemEntry::Dir { entries, .. } = nested_dir else {
            panic!("expected agent to be a Dir, got {nested_dir:?}");
        };
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            entries.get(&SafePath(PathBuf::from("data"))).unwrap(),
            FilesystemEntry::Dir { .. }
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

    #[test]
    fn write_overwrites_declared_and_leaves_everything_else() {
        let tmp_dir = TempDir::new().unwrap();

        // First write: A (top-level file), managed-dir with declared `old.txt`, projected map.
        let first_yaml = r#"
A.txt:
  kind: file
  text: hello
managed-dir:
  kind: dir
  entries:
    old.txt:
      kind: file
      text: from-config-1
projected:
  kind: dir_content_from_map
  source: ${nr-var:proj}
"#;
        let proj_first = HashMap::from([
            ("a.yaml".to_string(), Value::String("a-content".to_string())),
            ("b.yaml".to_string(), Value::String("b-content".to_string())),
        ]);
        let variables_first = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(tmp_dir.path().to_string_lossy()),
            ),
            (
                Namespace::Variable.namespaced_name("proj"),
                Variable::new(String::default(), false, None, Some(proj_first)),
            ),
        ]);

        serde_saphyr::from_str::<FileSystem>(first_yaml)
            .unwrap()
            .template_with(&variables_first)
            .unwrap()
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert!(tmp_dir.path().join("A.txt").exists());
        assert!(tmp_dir.path().join("managed-dir/old.txt").exists());
        assert!(tmp_dir.path().join("projected/a.yaml").exists());
        assert!(tmp_dir.path().join("projected/b.yaml").exists());

        // Sub-agent process writes runtime files that were never declared.
        let runtime_top = tmp_dir.path().join("agent-runtime.log");
        let runtime_in_dir = tmp_dir.path().join("managed-dir/cache.db");
        let runtime_in_projected = tmp_dir.path().join("projected/agent-state.log");
        std::fs::write(&runtime_top, "top-level runtime data").unwrap();
        std::fs::write(&runtime_in_dir, "cache").unwrap();
        std::fs::write(&runtime_in_projected, "state").unwrap();

        // Second write: A.txt removed; `old.txt` removed from managed-dir's entries; `b.yaml`
        // dropped from the projected map.
        let second_yaml = r#"
managed-dir:
  kind: dir
projected:
  kind: dir_content_from_map
  source: ${nr-var:proj}
"#;
        let proj_second = HashMap::from([(
            "a.yaml".to_string(),
            Value::String("a-content-v2".to_string()),
        )]);
        let variables_second = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(tmp_dir.path().to_string_lossy()),
            ),
            (
                Namespace::Variable.namespaced_name("proj"),
                Variable::new(String::default(), false, None, Some(proj_second)),
            ),
        ]);

        serde_saphyr::from_str::<FileSystem>(second_yaml)
            .unwrap()
            .template_with(&variables_second)
            .unwrap()
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        // No pruning: previously-declared, no-longer-declared paths remain on disk.
        assert!(
            tmp_dir.path().join("A.txt").exists(),
            "A.txt should survive: write never deletes previously-declared paths"
        );
        assert!(
            tmp_dir.path().join("managed-dir/old.txt").exists(),
            "old.txt inside managed-dir should survive"
        );
        assert!(
            !tmp_dir.path().join("projected/b.yaml").exists(),
            "projected/b.yaml is removed on re-write: dir_content_from_map clears the dir first"
        );
        // Currently-declared paths are present and updated.
        assert_eq!(
            std::fs::read_to_string(tmp_dir.path().join("projected/a.yaml")).unwrap(),
            "a-content-v2"
        );
        assert!(tmp_dir.path().join("managed-dir").is_dir());
        // Agent-process-created files survive everywhere.
        assert!(
            runtime_top.exists(),
            "top-level runtime file should survive"
        );
        assert_eq!(
            std::fs::read_to_string(&runtime_top).unwrap(),
            "top-level runtime data"
        );
        assert!(
            runtime_in_dir.exists(),
            "agent-created file inside a declared dir should survive"
        );
        assert_eq!(std::fs::read_to_string(&runtime_in_dir).unwrap(), "cache");
        assert!(
            !runtime_in_projected.exists(),
            "agent-created file inside dir_content_from_map is removed on re-write: \
             the dir is cleared before new map contents are written"
        );
    }

    /// Re-writing a `dir_content_from_map` entry with a different map removes files from the
    /// previous map that are absent in the new one. This prevents stale config files from lingering
    /// after a remote config update that removes an entry from the map variable.
    #[test]
    fn dir_content_from_map_write_removes_stale_files_on_rewrite() {
        let tmp_dir = TempDir::new().unwrap();
        let base = tmp_dir.path();

        let v1_yaml = r#"
logging.d:
  kind: dir_content_from_map
  source: ${nr-var:logs}
"#;
        let v2_yaml = v1_yaml;

        let variables_v1 = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(base.to_string_lossy()),
            ),
            (
                Namespace::Variable.namespaced_name("logs"),
                Variable::new(
                    String::default(),
                    false,
                    None,
                    Some(HashMap::from([(
                        "file-1.conf".to_string(),
                        Value::String("config-1".to_string()),
                    )])),
                ),
            ),
        ]);

        serde_saphyr::from_str::<FileSystem>(v1_yaml)
            .unwrap()
            .template_with(&variables_v1)
            .unwrap()
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert!(
            base.join("logging.d/file-1.conf").exists(),
            "first write must create the file"
        );

        // Second write: only file-2.conf in the map; file-1.conf is gone.
        let variables_v2 = Variables::from_iter(vec![
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(base.to_string_lossy()),
            ),
            (
                Namespace::Variable.namespaced_name("logs"),
                Variable::new(
                    String::default(),
                    false,
                    None,
                    Some(HashMap::from([(
                        "file-2.conf".to_string(),
                        Value::String("config-2".to_string()),
                    )])),
                ),
            ),
        ]);

        serde_saphyr::from_str::<FileSystem>(v2_yaml)
            .unwrap()
            .template_with(&variables_v2)
            .unwrap()
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert!(
            !base.join("logging.d/file-1.conf").exists(),
            "file removed from the map must be deleted on re-write"
        );
        assert!(
            base.join("logging.d/file-2.conf").exists(),
            "file added to the map must be present after re-write"
        );
    }

    /// Builds the reserved variable holding the shared filesystem base dir.
    fn shared_variables(shared_dir: &Path, remote_dir: &Path) -> Variables {
        Variables::from_iter(vec![
            (
                Namespace::SubAgent
                    .namespaced_name(AgentAttributes::VARIABLE_SHARED_FILESYSTEM_DIR),
                Variable::new_final_string_variable(shared_dir.to_string_lossy()),
            ),
            (
                Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_REMOTE_DIR),
                Variable::new_final_string_variable(remote_dir.to_string_lossy()),
            ),
        ])
    }

    /// The shared filesystem renders against `${nr-sub:shared_filesystem_dir}` and writes its tree there
    #[test]
    fn shared_filesystem_renders_and_writes_to_shared_base() {
        let tmp_dir = TempDir::new().unwrap();
        let shared = tmp_dir.path().join("shared-filesystem");
        let yaml = r#"
infra-agent-ohi-configs:
  kind: dir
  entries:
    nri-redis.yaml:
      kind: file
      text: "integration: redis"
"#;
        serde_saphyr::from_str::<SharedFileSystem>(yaml)
            .unwrap()
            .template_with(&shared_variables(&shared, tmp_dir.path()))
            .unwrap()
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(shared.join("infra-agent-ohi-configs/nri-redis.yaml")).unwrap(),
            "integration: redis"
        );
    }

    #[test]
    fn shared_filesystem_write_mutiple_times() {
        let tmp_dir = TempDir::new().unwrap();
        let shared = tmp_dir.path().join("shared-filesystem");
        let variables = shared_variables(&shared, tmp_dir.path());

        // Two sub-agents can write entries into the same shared base, both should remain.
        for (name, content) in [("agent-a.yaml", "a"), ("agent-b.yaml", "b")] {
            let yaml = format!("{name}:\n  kind: file\n  text: {content}");
            serde_saphyr::from_str::<SharedFileSystem>(&yaml)
                .unwrap()
                .template_with(&variables)
                .unwrap()
                .write(&LocalFile, &DirectoryManagerFs)
                .unwrap();
        }

        assert!(shared.join("agent-a.yaml").exists());
        assert!(shared.join("agent-b.yaml").exists());
    }

    #[test]
    fn shared_filesystem_supports_copy_from_file() {
        let tmp_dir = TempDir::new().unwrap();
        let remote = tmp_dir.path();
        let shared = remote.join("shared-filesystem");
        let source = remote.join("packages").join("nri-redis");
        let source_bytes = [0xFFu8, 0x00, b'b', b'i', b'n'];
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, source_bytes).unwrap();

        // Built from structs (not YAML) to keep the source path platform-native.
        let shared_fs = SharedFileSystem(FileSystem(HashMap::from([(
            PathBuf::from("nri-redis").try_into().unwrap(),
            FilesystemEntry::File {
                text: None,
                copy_from_file: Some(TemplateableValue::from_template(
                    source.to_string_lossy().to_string(),
                )),
            },
        )])));

        shared_fs
            .template_with(&shared_variables(&shared, remote))
            .unwrap()
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert_eq!(
            std::fs::read(shared.join("nri-redis")).unwrap(),
            source_bytes
        );
    }

    /// An empty (unused) shared filesystem renders even when `shared_filesystem_dir` is absent.
    #[test]
    fn empty_shared_filesystem_renders_without_shared_dir_variable() {
        SharedFileSystem::default()
            .template_with(&Variables::default())
            .expect("empty shared filesystem must render without the shared dir variable")
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();
    }

    /// `declared_paths` reports every `kind: file` (including files nested in co-owned directories)
    /// as an individually-owned file, every `kind: dir_content_from_map` as a whole-directory
    /// owner, and every `kind: dir` node (including nested ones) as a co-owned directory.
    #[test]
    fn declared_paths_splits_files_and_managed_dirs() {
        let yaml = r#"
top-file:
  kind: file
  text: hi
co-owned:
  kind: dir
  entries:
    nri-redis.yaml:
      kind: file
      text: "integration: redis"
    nested:
      kind: dir
      entries:
        deep.yaml:
          kind: file
          text: deep
projected:
  kind: dir_content_from_map
  source: ${nr-var:m}
"#;
        let shared = serde_saphyr::from_str::<SharedFileSystem>(yaml).unwrap();
        // A real temp dir gives an absolute, platform-native base (path literals like `/shared` are
        // not absolute on Windows). `declared_paths` never touches disk, so the dir need not exist.
        let tmp_dir = TempDir::new().unwrap();
        let base = tmp_dir.path();

        let declared = shared.declared_paths(base);

        assert_eq!(
            declared.owned_files,
            HashSet::from([
                base.join("top-file"),
                base.join("co-owned").join("nri-redis.yaml"),
                base.join("co-owned").join("nested").join("deep.yaml"),
            ]),
            "every `kind: file` must be reported, and the co-owned dir path must not be"
        );
        assert_eq!(
            declared.owned_dirs,
            HashSet::from([base.join("projected")]),
            "`dir_content_from_map` must be reported as a whole-directory owner"
        );
        assert_eq!(
            declared.dirs,
            HashSet::from([base.join("co-owned"), base.join("co-owned").join("nested"),]),
            "every `kind: dir` node, including nested ones, must be reported as co-owned"
        );
    }

    /// A shared filesystem with no entries declares no owned paths.
    #[test]
    fn declared_paths_of_empty_is_empty() {
        let tmp_dir = TempDir::new().unwrap();
        let declared = SharedFileSystem::default().declared_paths(tmp_dir.path());
        assert!(declared.owned_files.is_empty());
        assert!(declared.owned_dirs.is_empty());
        assert!(declared.dirs.is_empty());
    }
}
