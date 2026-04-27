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

use crate::{
    agent_control::config::Registry,
    agent_type::runtime_config::on_host::package::rendered::{Repository, Version},
};

/// NAME_TOTAL_LENGTH_MAX is the maximum total number of characters in a repository name.
const DOCKER_HUB_DOMAIN_LEGACY: &str = "index.docker.io";
const DOCKER_HUB_DOMAIN: &str = "docker.io";
const DOCKER_HUB_OFFICIAL_REPO_NAME: &str = "library";
const DEFAULT_TAG: &str = "latest";

#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceParser(Reference);

impl From<(&Registry, &Repository, &Version)> for ReferenceParser {
    fn from((registry, repository, version): (&Registry, &Repository, &Version)) -> Self {
        let registry = registry.to_string();
        let repository = repository.to_string();
        let reference = match version.tag_and_digest() {
            (Some(tag), Some(digest)) => {
                Reference::with_tag_and_digest(registry, repository, tag, digest)
            }
            (Some(tag), None) => Reference::with_tag(registry.clone(), repository.clone(), tag),
            (None, Some(digest)) => {
                Reference::with_digest(registry.clone(), repository.clone(), digest)
            }
            (None, None) => {
                Reference::with_tag(registry.clone(), repository.clone(), DEFAULT_TAG.to_owned())
            }
        };

        Self(reference)
    }
}

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

        let registry =
            Registry::from_str(&registry).map_err(|_| ParseError::ReferenceInvalidFormat)?;
        let repository =
            Repository::from_str(&repository).map_err(|_| ParseError::ReferenceInvalidFormat)?;

        let version = match (tag, digest) {
            (Some(t), Some(d)) => format!("{}@{}", t, d),
            (Some(t), None) => t.to_owned(),
            (None, Some(d)) => format!("@{}", d),
            (None, None) => DEFAULT_TAG.to_owned(),
        };
        let version =
            Version::from_str(&version).map_err(|_| ParseError::ReferenceInvalidFormat)?;

        Ok(Self::from((&registry, &repository, &version)))
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

#[cfg(test)]
mod test {
    use super::*;

    mod parse {
        use super::*;
        use rstest::rstest;

        #[rstest]
        #[case("busybox")]
        #[case("test.com:tag")]
        #[case("test.com:5000")]
        #[case("test.com/repo:tag")]
        #[case("test:5000/repo")]
        #[case("test:5000/repo:tag")]
        #[case(
            "test:5000/repo@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        )]
        #[case(
            "test:5000/repo:tag@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        )]
        #[case("lowercase:Uppercase")]
        #[case("sub-dom1.foo.com/bar/baz/quux")]
        #[case("sub-dom1.foo.com/bar/baz/quux:some-long-tag")]
        #[case("b.gcr.io/test.example.com/my-app:test.example.com")]
        // ☃.com in punycode
        #[case("xn--n3h.com/myimage:xn--n3h.com")]
        // 🐳.com in punycode
        #[case(
            "xn--7o8h.com/myimage:xn--7o8h.com@sha512:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        )]
        #[case("foo_bar.com:8080")]
        #[case("foo/foo_bar.com:8080")]
        #[case("opensuse/leap:15.3")]
        fn parse_good_reference(#[case] input: &str) {
            let expected_reference = Reference::from_str(input).unwrap();
            let actual_reference = Reference::from(
                ReferenceParser::try_from(input).expect("could not parse reference"),
            );
            assert_eq!(expected_reference, actual_reference);
        }

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
        fn parse_from_components(
            #[case] registry: &str,
            #[case] repository: &str,
            #[case] version: &str,
            #[case] whole: &str,
        ) {
            let registry = Registry::from_str(registry).unwrap();
            let repository = Repository::from_str(repository).unwrap();
            let version = Version::from_str(version).unwrap();
            let reference =
                Reference::from(ReferenceParser::from((&registry, &repository, &version)));
            assert_eq!(whole, reference.whole());
        }

        #[rstest]
        #[case("", ParseError::NameEmpty)]
        #[case(":justtag", ParseError::ReferenceInvalidFormat)]
        #[case(
            "@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ParseError::ReferenceInvalidFormat
        )]
        #[case("aa/asdf$$^/aa", ParseError::ReferenceInvalidFormat)]
        fn parse_bad_reference(#[case] input: &str, #[case] err: ParseError) {
            assert_eq!(ReferenceParser::try_from(input).unwrap_err(), err)
        }

        #[rstest]
        #[case(
            "busybox",
            "docker.io",
            "index.docker.io",
            "docker.io/library/busybox:latest"
        )]
        #[case("test.com/repo:tag", "test.com", "test.com", "test.com/repo:tag")]
        #[case("test:5000/repo", "test:5000", "test:5000", "test:5000/repo:latest")]
        #[case(
            "sub-dom1.foo.com/bar/baz/quux",
            "sub-dom1.foo.com",
            "sub-dom1.foo.com",
            "sub-dom1.foo.com/bar/baz/quux:latest"
        )]
        #[case(
            "b.gcr.io/test.example.com/my-app:test.example.com",
            "b.gcr.io",
            "b.gcr.io",
            "b.gcr.io/test.example.com/my-app:test.example.com"
        )]
        fn test_mirror_registry(
            #[case] input: &str,
            #[case] registry: &str,
            #[case] resolved_registry: &str,
            #[case] whole: &str,
        ) {
            let mut reference = Reference::from(
                ReferenceParser::try_from(input).expect("could not parse reference"),
            );
            assert_eq!(resolved_registry, reference.resolve_registry());
            assert_eq!(registry, reference.registry());
            assert_eq!(None, reference.namespace());
            assert_eq!(whole, reference.whole());

            reference.set_mirror_registry("docker.mirror.io".to_owned());
            assert_eq!("docker.mirror.io", reference.resolve_registry());
            assert_eq!(registry, reference.registry());
            assert_eq!(Some(registry), reference.namespace());
            assert_eq!(whole, reference.whole());
        }
    }
}
