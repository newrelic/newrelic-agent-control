//! Helpers for deserializing YAML documents.

use serde::de::DeserializeOwned;

/// Deserializes a YAML document, treating an empty document as an empty mapping (`{}`).
///
/// `serde-saphyr` 2.1.0 rejects empty documents (and documents that only contain whitespace,
/// comments or document markers) when the target is a mapping or a sequence, instead of producing
/// the empty value. Agent Control relies on the latter behavior: an empty remote configuration is
/// the signal to reset a sub-agent to its local configuration, and local configuration files may
/// be empty.
///
/// Empty documents are told apart from malformed YAML by parsing the document untyped only when
/// the typed parse fails: empty and explicit `null` documents are the only ones that deserialize
/// to [`serde_json::Value::Null`], while truncated or otherwise invalid documents fail to parse.
///
/// # Errors
/// Returns the [`serde_saphyr::Error`] from the typed parse if the document is not valid YAML.
pub fn from_str_or_empty_mapping<T>(value: &str) -> Result<T, serde_saphyr::Error>
where
    T: DeserializeOwned,
{
    match serde_saphyr::from_str(value) {
        Ok(parsed) => Ok(parsed),
        Err(err) => match serde_saphyr::from_str::<serde_json::Value>(value) {
            Ok(serde_json::Value::Null) => serde_saphyr::from_str("{}"),
            _ => Err(err),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::Value;

    use super::from_str_or_empty_mapping;

    #[test]
    fn empty_documents_deserialize_to_an_empty_mapping() {
        for document in [
            "",
            "  \n\t\n",
            "# only a comment\n",
            "# first comment\n\n# second comment\n",
            "---\n",
            "---\n# comment\n...\n",
            "null",
            "~",
        ] {
            let parsed = from_str_or_empty_mapping::<HashMap<String, Value>>(document).unwrap();
            assert!(
                parsed.is_empty(),
                "expected an empty config for document {document:?}"
            );
        }
    }

    #[test]
    fn non_empty_documents_are_parsed() {
        let parsed = from_str_or_empty_mapping::<HashMap<String, Value>>("key: value").unwrap();
        assert_eq!(parsed.get("key"), Some(&Value::String("value".to_string())));
    }

    #[test]
    fn invalid_documents_still_error() {
        for document in ["key: [", "key: {", "key: \"unterminated", "- not a mapping"] {
            let parsed = from_str_or_empty_mapping::<HashMap<String, Value>>(document);
            assert!(parsed.is_err(), "expected {document:?} to fail parsing");
        }
    }
}
