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

#[cfg(test)]
mod test {
    use super::*;

    mod parse {
        use super::*;
        use rstest::rstest;

        #[rstest(input, registry, repository, tag, digest, whole,
            case("busybox", "docker.io", "library/busybox", Some("latest"), None, "docker.io/library/busybox:latest"),
            case("test.com:tag", "docker.io", "library/test.com", Some("tag"), None, "docker.io/library/test.com:tag"),
            case("test.com:5000", "docker.io", "library/test.com", Some("5000"), None, "docker.io/library/test.com:5000"),
            case("test.com/repo:tag", "test.com", "repo", Some("tag"), None, "test.com/repo:tag"),
            case("test:5000/repo", "test:5000", "repo", Some("latest"), None, "test:5000/repo:latest"),
            case("test:5000/repo:tag", "test:5000", "repo", Some("tag"), None, "test:5000/repo:tag"),
            case("test:5000/repo@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "test:5000", "repo", None, Some("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "test:5000/repo@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            case("test:5000/repo:tag@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "test:5000", "repo", Some("tag"), Some("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "test:5000/repo:tag@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            case("lowercase:Uppercase", "docker.io", "library/lowercase", Some("Uppercase"), None, "docker.io/library/lowercase:Uppercase"),
            case("sub-dom1.foo.com/bar/baz/quux", "sub-dom1.foo.com", "bar/baz/quux", Some("latest"), None, "sub-dom1.foo.com/bar/baz/quux:latest"),
            case("sub-dom1.foo.com/bar/baz/quux:some-long-tag", "sub-dom1.foo.com", "bar/baz/quux", Some("some-long-tag"), None, "sub-dom1.foo.com/bar/baz/quux:some-long-tag"),
            case("b.gcr.io/test.example.com/my-app:test.example.com", "b.gcr.io", "test.example.com/my-app", Some("test.example.com"), None, "b.gcr.io/test.example.com/my-app:test.example.com"),
            // ☃.com in punycode
            case("xn--n3h.com/myimage:xn--n3h.com", "xn--n3h.com", "myimage", Some("xn--n3h.com"), None, "xn--n3h.com/myimage:xn--n3h.com"),
            // 🐳.com in punycode
            case("xn--7o8h.com/myimage:xn--7o8h.com@sha512:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "xn--7o8h.com", "myimage", Some("xn--7o8h.com"), Some("sha512:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "xn--7o8h.com/myimage:xn--7o8h.com@sha512:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            case("foo_bar.com:8080", "docker.io", "library/foo_bar.com", Some("8080"), None, "docker.io/library/foo_bar.com:8080" ),
            case("foo/foo_bar.com:8080", "docker.io", "foo/foo_bar.com", Some("8080"), None, "docker.io/foo/foo_bar.com:8080"),
            case("opensuse/leap:15.3", "docker.io", "opensuse/leap", Some("15.3"), None, "docker.io/opensuse/leap:15.3"),
        )]
        fn parse_good_reference(
            input: &str,
            registry: &str,
            repository: &str,
            tag: Option<&str>,
            digest: Option<&str>,
            whole: &str,
        ) {
            println!("input: {}", input);
            let reference = Reference::from(
                ReferenceParser::try_from(input).expect("could not parse reference"),
            );
            println!("{} -> {:?}", input, reference);
            assert_eq!(registry, reference.registry());
            assert_eq!(repository, reference.repository());
            assert_eq!(tag, reference.tag());
            assert_eq!(digest, reference.digest());
            assert_eq!(whole, reference.whole());
        }

        #[rstest(input, err,
            case("", ParseError::NameEmpty),
            case(":justtag", ParseError::ReferenceInvalidFormat),
            case("@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", ParseError::ReferenceInvalidFormat),
            case("repo@sha256:ffffffffffffffffffffffffffffffffff", ParseError::DigestInvalidLength),
            case("validname@invaliddigest:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", ParseError::DigestUnsupported),
            case("Uppercase:tag", ParseError::NameContainsUppercase),
            // FIXME: "Uppercase" is incorrectly handled as a domain-name here, and therefore passes.
            // https://github.com/docker/distribution/blob/master/reference/reference_test.go#L104-L109
            // case("Uppercase/lowercase:tag", ParseError::NameContainsUppercase),
            case("test:5000/Uppercase/lowercase:tag", ParseError::NameContainsUppercase),
            case("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", ParseError::NameTooLong),
            case("aa/asdf$$^/aa", ParseError::ReferenceInvalidFormat)
        )]
        fn parse_bad_reference(input: &str, err: ParseError) {
            assert_eq!(ReferenceParser::try_from(input).unwrap_err(), err)
        }

        #[rstest(
            input,
            registry,
            resolved_registry,
            whole,
            case(
                "busybox",
                "docker.io",
                "index.docker.io",
                "docker.io/library/busybox:latest"
            ),
            case("test.com/repo:tag", "test.com", "test.com", "test.com/repo:tag"),
            case("test:5000/repo", "test:5000", "test:5000", "test:5000/repo:latest"),
            case(
                "sub-dom1.foo.com/bar/baz/quux",
                "sub-dom1.foo.com",
                "sub-dom1.foo.com",
                "sub-dom1.foo.com/bar/baz/quux:latest"
            ),
            case(
                "b.gcr.io/test.example.com/my-app:test.example.com",
                "b.gcr.io",
                "b.gcr.io",
                "b.gcr.io/test.example.com/my-app:test.example.com"
            )
        )]
        fn test_mirror_registry(input: &str, registry: &str, resolved_registry: &str, whole: &str) {
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

        #[rstest(
            expected,
            registry,
            repository,
            tag,
            digest,
            case(
                "docker.io/foo/bar:1.2@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "docker.io",
                "foo/bar",
                "1.2",
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            )
        )]
        fn test_create_reference_from_tag_and_digest(
            expected: &str,
            registry: &str,
            repository: &str,
            tag: &str,
            digest: &str,
        ) {
            let reference = Reference::with_tag_and_digest(
                registry.to_string(),
                repository.to_string(),
                tag.to_string(),
                digest.to_string(),
            );
            assert_eq!(expected, reference.to_string());
        }
    }
}
