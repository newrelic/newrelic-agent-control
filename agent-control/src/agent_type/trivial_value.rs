use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TrivialValue {
    String(String),
    #[serde(skip)]
    File(FilePathWithContent),
    #[serde(skip)]
    Yaml(serde_yaml::Value),
    Bool(bool),
    Number(serde_yaml::Number),
    #[serde(skip)]
    MapStringString(Map<String, String>),
    #[serde(skip)]
    MapStringYaml(Map<String, serde_yaml::Value>),
    #[serde(skip)]
    MapStringFile(Map<String, FilePathWithContent>),
}

impl TrivialValue {
    /// If the trivial value is a yaml, it returns a copy the corresponding [serde_yaml::Value], returns None otherwise.
    pub fn to_yaml_value(&self) -> Option<serde_yaml::Value> {
        match self {
            Self::Yaml(yaml) => Some(yaml.clone()),
            _ => None,
        }
    }

    pub fn as_file(&self) -> Option<&FilePathWithContent> {
        match self {
            Self::File(file) => Some(file),
            _ => None,
        }
    }

    pub fn as_map_string_file(&self) -> Option<&Map<String, FilePathWithContent>> {
        match self {
            Self::MapStringFile(map) => Some(map),
            _ => None,
        }
    }
}

impl Display for TrivialValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrivialValue::String(s) => write!(f, "{s}"),
            TrivialValue::File(file) => write!(f, "{}", file.path.to_string_lossy()),
            TrivialValue::Yaml(yaml) => write!(
                f,
                "{}",
                serde_yaml::to_string(yaml)
                    .expect("A value of type serde_yaml::Value should always be serializable")
            ),
            TrivialValue::Bool(b) => write!(f, "{b}"),
            TrivialValue::Number(n) => write!(f, "{n}"),
            TrivialValue::MapStringString(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
            TrivialValue::MapStringYaml(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    .map(|(key, value)| {
                        let value = serde_yaml::to_string(value).expect(
                            "A value of type serde_yaml::Value should always be serializable",
                        );
                        format!("{key}={value}")
                    })
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
            TrivialValue::MapStringFile(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    .map(|(key, value)| format!("{key}={}", value.path.to_string_lossy()))
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
        }
    }
}

/// Represents a file path and its content.
#[derive(Debug, PartialEq, Default, Clone, Deserialize, Serialize)]
#[serde(from = "String")]
#[serde(into = "String")]
pub struct FilePathWithContent {
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(flatten)]
    pub content: String,
}

impl FilePathWithContent {
    pub fn new(path: PathBuf, content: String) -> Self {
        FilePathWithContent { path, content }
    }
    pub fn with_path(&mut self, path: PathBuf) {
        self.path = path;
    }
}

// The minimum information needed to create a FilePathWithContent is the contents
impl From<String> for FilePathWithContent {
    fn from(content: String) -> Self {
        FilePathWithContent {
            content,
            ..Default::default()
        }
    }
}

impl From<FilePathWithContent> for String {
    fn from(file: FilePathWithContent) -> Self {
        file.content
    }
}

#[cfg(test)]
mod tests {
    use super::FilePathWithContent;

    #[test]
    fn test_file_path_with_contents() {
        let file = FilePathWithContent::new("path".into(), "file_content".to_string());
        assert_eq!(serde_yaml::to_string(&file).unwrap(), "file_content\n");
    }
}
