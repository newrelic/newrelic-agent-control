//! This module contains an alternative implementation of `impl TryFrom<&str> for Reference` behind
//! a new type that we use to create values.
//!
//! The implementation has been proposed upstream, but this way, we can keep using the library
//! without resorting to a fork while the proposed changes are discussed and land.
//!
//! Some existing private constants and functions have been copied from upstream.
//!
//! # ⚠️ DELETE ME ‼️
//!
//! This module must be entirely removed from the codebase when
//! [oci-spec-rs#322](https://github.com/youki-dev/oci-spec-rs/pull/322) is addressed!

use std::str::FromStr;

use oci_client::{ParseError, Reference};

/// NAME_TOTAL_LENGTH_MAX is the maximum total number of characters in a repository name.
const NAME_TOTAL_LENGTH_MAX: usize = 255;
const DOCKER_HUB_DOMAIN_LEGACY: &str = "index.docker.io";
const DOCKER_HUB_DOMAIN: &str = "docker.io";
const DOCKER_HUB_OFFICIAL_REPO_NAME: &str = "library";
const DEFAULT_TAG: &str = "latest";

#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceParser(Reference);

impl FromStr for ReferenceParser {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl TryFrom<&str> for ReferenceParser {
    type Error = ParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(ParseError::NameEmpty);
        }
        // A bare ':' or '@' prefix has no name component.
        if s.starts_with(':') || s.starts_with('@') {
            return Err(ParseError::ReferenceInvalidFormat);
        }

        // Extract the digest (`@<algo>:<hex>`).
        let (name_and_tag, digest) = match s.split_once('@') {
            Some((n, d)) => (n, Some(d)),
            None => (s, None),
        };

        // Extract the tag.
        let (name, tag) = split_name_tag(name_and_tag);

        // Get registry / repository.
        let (registry, repository) = split_domain(name);

        // Length check (repository only).
        if repository.len() > NAME_TOTAL_LENGTH_MAX {
            return Err(ParseError::NameTooLong);
        }

        // Character validation.
        validate_repository(&repository)?;
        if let Some(d) = digest {
            validate_digest(d)?;
        }

        let reference = match (tag, digest) {
            (Some(t), Some(d)) => {
                Reference::with_tag_and_digest(registry, repository, t.to_owned(), d.to_owned())
            }
            (Some(t), None) => Reference::with_tag(registry, repository, t.to_owned()),
            (None, Some(d)) => Reference::with_digest(registry, repository, d.to_owned()),
            (None, None) => Reference::with_tag(registry, repository, DEFAULT_TAG.to_owned()),
        };
        Ok(Self(reference))
    }
}

impl TryFrom<String> for ReferenceParser {
    type Error = ParseError;
    fn try_from(string: String) -> Result<Self, Self::Error> {
        Self::try_from(string.as_str())
    }
}

impl From<ReferenceParser> for Reference {
    fn from(value: ReferenceParser) -> Self {
        value.0
    }
}

/// Splits a repository name to domain and remotename string.
/// If no valid domain is found, the default domain is used. Repository name
/// needs to be already validated before.
///
/// This function is a Rust rewrite of the official Go code used by Docker:
/// https://github.com/distribution/distribution/blob/41a0452eea12416aaf01bceb02a924871e964c67/reference/normalize.go#L87-L104
fn split_domain(name: &str) -> (String, String) {
    let mut domain: String;
    let mut remainder: String;

    match name.split_once('/') {
        None => {
            domain = DOCKER_HUB_DOMAIN.into();
            remainder = name.into();
        }
        Some((left, right)) => {
            if !(left.contains('.') || left.contains(':')) && left != "localhost" {
                domain = DOCKER_HUB_DOMAIN.into();
                remainder = name.into();
            } else {
                domain = left.into();
                remainder = right.into();
            }
        }
    }
    if domain == DOCKER_HUB_DOMAIN_LEGACY {
        domain = DOCKER_HUB_DOMAIN.into();
    }
    if domain == DOCKER_HUB_DOMAIN && !remainder.contains('/') {
        remainder = format!("{DOCKER_HUB_OFFICIAL_REPO_NAME}/{remainder}");
    }

    (domain, remainder)
}

/// Split `name[:tag]` into `(name, Option<tag>)`.
///
/// A `:` is treated as a tag separator only when it appears after the last `/`
/// (or when there is no `/`), so that `host:port/repo` is parsed correctly.
fn split_name_tag(s: &str) -> (&str, Option<&str>) {
    let last_slash = s.rfind('/');
    let last_colon = s.rfind(':');
    match (last_slash, last_colon) {
        (_, None) => (s, None),
        (None, Some(c)) => (&s[..c], Some(&s[c + 1..])),
        (Some(sl), Some(c)) if c > sl => (&s[..c], Some(&s[c + 1..])),
        _ => (s, None), // colon belongs to host:port — not a tag
    }
}

/// Validate that every path component of the repository contains only `[a-z0-9._-]`.
fn validate_repository(repo: &str) -> Result<(), ParseError> {
    repo.split('/').try_for_each(|component| {
        if !component.is_empty() {
            component.chars().try_for_each(validate_component_char)
        } else {
            Err(ParseError::ReferenceInvalidFormat)
        }
    })
}

fn validate_component_char(c: char) -> Result<(), ParseError> {
    if c.is_ascii_uppercase() {
        Err(ParseError::NameContainsUppercase)
    } else if !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-' {
        Err(ParseError::ReferenceInvalidFormat)
    } else {
        Ok(())
    }
}

/// Validate a digest string of the form `<algorithm>:<hex>`.
fn validate_digest(digest: &str) -> Result<(), ParseError> {
    use ParseError::*;
    match digest.split_once(':') {
        Some(("sha256", hex)) if hex.len() == 64 => Ok(()),
        Some(("sha384", hex)) if hex.len() == 96 => Ok(()),
        Some(("sha512", hex)) if hex.len() == 128 => Ok(()),
        Some(("sha256", _)) | Some(("sha384", _)) | Some(("sha512", _)) => Err(DigestInvalidLength),
        Some(_) => Err(DigestUnsupported),
        None => Err(DigestInvalidFormat),
    }
}
