use std::{fmt::Display, str::FromStr, time::Duration};

use oci_client::Reference;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::agent_control::config::Registry;

const REPOSITORY_TOTAL_LENGTH_MAX: usize = 255;
const TAG_TOTAL_LENGTH_MAX: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// Download defines the supported repository sources for the packages.
    pub download: Download,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// OCI repository definition
    pub oci: Oci,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Oci {
    pub repository: Repository,
    pub version: Version,
    pub public_key_url: Option<Url>,
    pub postdownload: Option<Postdownload>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Postdownload {
    /// Arguments where first element is the command/executable, followed by arguments.
    /// Example: ["bash", "./scripts/postdownload.sh", "-e"]
    pub args: Vec<String>,
    /// Environmental variables
    pub env: std::collections::HashMap<String, String>,
    /// Timeout duration
    pub timeout: Duration,
}

const DEFAULT_TAG: &str = "latest";

impl Oci {
    pub fn to_reference(&self, registry: &Registry) -> Reference {
        let registry_str = registry.to_string();
        let repository_str = self.repository.to_string();

        match self.version.tag_and_digest() {
            (Some(tag), Some(digest)) => {
                Reference::with_tag_and_digest(registry_str, repository_str, tag, digest)
            }
            (Some(tag), None) => Reference::with_tag(registry_str, repository_str, tag),
            (None, Some(digest)) => Reference::with_digest(registry_str, repository_str, digest),
            (None, None) => {
                Reference::with_tag(registry_str, repository_str, DEFAULT_TAG.to_owned())
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct InvalidRepository(String);

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Repository(String);

impl Display for Repository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// Check validation rules from https://github.com/opencontainers/distribution-spec/blob/main/spec.md#pull
impl FromStr for Repository {
    type Err = InvalidRepository;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(InvalidRepository(
                "repository name cannot be empty".to_string(),
            ));
        }

        if s.len() > REPOSITORY_TOTAL_LENGTH_MAX {
            return Err(InvalidRepository("repository name too long".to_string()));
        }

        // Validate that every path component of the repository contains only `[a-z0-9._-]`.
        s.split('/').try_for_each(|component| {
            if component.is_empty() {
                return Err(InvalidRepository(
                    "repository name contains empty component".to_string(),
                ));
            }

            component.chars().try_for_each(validate_repository_char)
        })?;

        Ok(Repository(s.to_string()))
    }
}

fn validate_repository_char(c: char) -> Result<(), InvalidRepository> {
    if c.is_ascii_uppercase() {
        Err(InvalidRepository(
            "repository name contains uppercase character".to_string(),
        ))
    } else if !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-' {
        Err(InvalidRepository(
            "repository name contains invalid character".to_string(),
        ))
    } else {
        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct InvalidVersion(String);

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Version(String);

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// Check validation rules from
//
// * https://github.com/opencontainers/distribution-spec/blob/main/spec.md#pulling-manifests
// * https://github.com/opencontainers/image-spec/blob/main/descriptor.md#digests
impl FromStr for Version {
    type Err = InvalidVersion;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.ends_with('@') {
            return Err(InvalidVersion("version cannot end with '@'".to_string()));
        }

        if s.contains('@') {
            let (tag, digest) = s
                .split_once('@')
                .expect("split_once should succeed since we checked contains('@')");

            if !tag.is_empty() {
                if tag.len() > TAG_TOTAL_LENGTH_MAX {
                    return Err(InvalidVersion("version tag too long".to_string()));
                }

                tag.chars().try_for_each(validate_version_tag_char)?;
            }

            validate_digest(digest)?;
        } else {
            if s.len() > TAG_TOTAL_LENGTH_MAX {
                return Err(InvalidVersion("version tag too long".to_string()));
            }

            s.chars().try_for_each(validate_version_tag_char)?;
        }

        Ok(Version(s.to_string()))
    }
}

/// Validate that the version tag contains only `[a-zA-Z0-9_][a-zA-Z0-9._-]`.
fn validate_version_tag_char(c: char) -> Result<(), InvalidVersion> {
    if !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-' {
        Err(InvalidVersion(
            "version tag contains invalid character".to_string(),
        ))
    } else {
        Ok(())
    }
}

/// Validate a digest string of the form `<algorithm>:<hex>`.
fn validate_digest(digest: &str) -> Result<(), InvalidVersion> {
    match digest.split_once(':') {
        Some(("sha256", hex)) if hex.len() == 64 => Ok(()),
        Some(("sha384", hex)) if hex.len() == 96 => Ok(()),
        Some(("sha512", hex)) if hex.len() == 128 => Ok(()),
        Some(("sha256", _)) | Some(("sha384", _)) | Some(("sha512", _)) => {
            Err(InvalidVersion("digest has invalid length".to_string()))
        }
        Some(_) => Err(InvalidVersion("digest algorithm unsupported".to_string())),
        None => Err(InvalidVersion("digest has invalid format".to_string())),
    }
}

impl Version {
    pub fn tag_and_digest(&self) -> (Option<String>, Option<String>) {
        if self.0.is_empty() {
            return (None, None);
        }

        match self.0.split_once('@') {
            Some(("", digest)) => (None, Some(digest.to_string())),
            Some((tag, "")) => (Some(tag.to_string()), None), //This should never happen
            Some((tag, digest)) => (Some(tag.to_string()), Some(digest.to_string())),
            None => (Some(self.0.clone()), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use std::str::FromStr;

    use super::*;

    #[rstest]
    #[case("latest")]
    #[case("v1.0.0")]
    #[case("1.0.0-alpha")]
    #[case("@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")]
    #[case("v1.0.0@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")]
    fn test_valid_version(#[case] input: &str) {
        assert!(Version::from_str(input).is_ok());
    }

    #[rstest]
    #[case("v1.0.0@")]
    #[case("v1.0.0@sha256:123")]
    #[case("v1.0.0@invaliddigest")]
    fn test_invalid_version(#[case] input: &str) {
        assert!(Version::from_str(input).is_err());
    }

    #[rstest]
    #[case("repo")]
    #[case("myrepo/myimage")]
    #[case("myrepo-123/myimage_456/subdir")]
    fn test_valid_repository(#[case] input: &str) {
        assert!(Repository::from_str(input).is_ok());
    }

    #[rstest]
    #[case("")]
    #[case("InvalidRepo")]
    #[case("invalid/repo/")]
    #[case("invalid/repo//subdir")]
    #[case(String::from("a").repeat(REPOSITORY_TOTAL_LENGTH_MAX + 1))]
    fn test_invalid_repository(#[case] input: impl AsRef<str>) {
        assert!(Repository::from_str(input.as_ref()).is_err());
    }

    mod to_reference {
        use super::*;

        #[rstest]
        #[case::with_tag("docker.io", "nr/test", "v1.0.0", "docker.io/nr/test:v1.0.0")]
        #[case::with_digest(
            "docker.io",
            "nr/test",
            "@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "docker.io/nr/test@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        )]
        #[case::with_tag_and_digest(
            "docker.io",
            "nr/test",
            "v1.0.0@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "docker.io/nr/test:v1.0.0@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        )]
        #[case::without_tag_or_digest("docker.io", "nr/test", "", "docker.io/nr/test:latest")]
        fn test_to_reference(
            #[case] registry: &str,
            #[case] repository: &str,
            #[case] version: &str,
            #[case] expected_whole: &str,
        ) {
            let registry = Registry::from_str(registry).unwrap();
            let oci = Oci {
                repository: Repository::from_str(repository).unwrap(),
                version: Version::from_str(version).unwrap(),
                public_key_url: None,
                postdownload: None,
            };
            let reference = oci.to_reference(&registry);
            assert_eq!(expected_whole, reference.whole());
        }
    }
}
