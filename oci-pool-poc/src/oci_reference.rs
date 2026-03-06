use std::{fmt, str::FromStr};

use oci_spec::distribution::{ParseError, Reference};

const NAME_TOTAL_LENGTH_MAX: usize = 255;
const DOCKER_HUB_DOMAIN: &str = "docker.io";
const DOCKER_HUB_DOMAIN_LEGACY: &str = "index.docker.io";
const DOCKER_HUB_OFFICIAL_REPO_NAME: &str = "library";
const DEFAULT_TAG: &str = "latest";

/// A regex-free OCI image reference that wraps [`Reference`].
///
/// Parsing replicates the behaviour of `Reference::try_from` but uses plain
/// string operations instead of a compiled regex, avoiding the allocation and
/// initialisation cost of the regex engine.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OciReference(Reference);

impl OciReference {
    // ── constructor mirrors ────────────────────────────────────────────────

    pub fn with_tag(registry: String, repository: String, tag: String) -> Self {
        Self(Reference::with_tag(registry, repository, tag))
    }

    pub fn with_digest(registry: String, repository: String, digest: String) -> Self {
        Self(Reference::with_digest(registry, repository, digest))
    }

    pub fn with_tag_and_digest(
        registry: String,
        repository: String,
        tag: String,
        digest: String,
    ) -> Self {
        Self(Reference::with_tag_and_digest(registry, repository, tag, digest))
    }

    // ── accessor delegation ────────────────────────────────────────────────

    pub fn registry(&self) -> &str {
        self.0.registry()
    }

    pub fn repository(&self) -> &str {
        self.0.repository()
    }

    pub fn tag(&self) -> Option<&str> {
        self.0.tag()
    }

    pub fn digest(&self) -> Option<&str> {
        self.0.digest()
    }

    pub fn whole(&self) -> String {
        self.0.whole()
    }

    pub fn resolve_registry(&self) -> &str {
        self.0.resolve_registry()
    }

    pub fn set_mirror_registry(&mut self, registry: String) {
        self.0.set_mirror_registry(registry);
    }

    pub fn namespace(&self) -> Option<&str> {
        self.0.namespace()
    }
}

// ── Parsing ────────────────────────────────────────────────────────────────

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

/// Mirror of the `split_domain` function from `oci-spec`.
///
/// Splits a repository name into `(registry, repository)`, normalising
/// Docker Hub short-names to `docker.io/library/<name>`.
fn split_domain(name: &str) -> (String, String) {
    let (mut domain, mut remainder) = match name.split_once('/') {
        None => (DOCKER_HUB_DOMAIN.to_owned(), name.to_owned()),
        Some((left, _right)) => {
            if !(left.contains('.') || left.contains(':')) && left != "localhost" {
                // No domain-like prefix — treat the whole string as a Docker Hub path.
                (DOCKER_HUB_DOMAIN.to_owned(), name.to_owned())
            } else {
                let sep = left.len() + 1;
                (left.to_owned(), name[sep..].to_owned())
            }
        }
    };
    if domain == DOCKER_HUB_DOMAIN_LEGACY {
        domain = DOCKER_HUB_DOMAIN.to_owned();
    }
    if domain == DOCKER_HUB_DOMAIN && !remainder.contains('/') {
        remainder = format!("{DOCKER_HUB_OFFICIAL_REPO_NAME}/{remainder}");
    }
    (domain, remainder)
}

/// Validate that every path component of the repository contains only
/// `[a-z0-9._-]`.  Uppercase letters and other special characters are
/// rejected so that the same errors as the regex-based parser are produced.
fn validate_repository(repo: &str) -> Result<(), ParseError> {
    for component in repo.split('/') {
        if component.is_empty() {
            return Err(ParseError::ReferenceInvalidFormat);
        }
        for c in component.chars() {
            if c.is_ascii_uppercase() || (!c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-') {
                return Err(ParseError::ReferenceInvalidFormat);
            }
        }
    }
    Ok(())
}

/// Validate a digest string of the form `<algorithm>:<hex>`.
fn validate_digest(digest: &str) -> Result<(), ParseError> {
    match digest.split_once(':') {
        None => Err(ParseError::DigestInvalidFormat),
        Some(("sha256", hex)) => {
            if hex.len() != 64 { Err(ParseError::DigestInvalidLength) } else { Ok(()) }
        }
        Some(("sha384", hex)) => {
            if hex.len() != 96 { Err(ParseError::DigestInvalidLength) } else { Ok(()) }
        }
        Some(("sha512", hex)) => {
            if hex.len() != 128 { Err(ParseError::DigestInvalidLength) } else { Ok(()) }
        }
        Some(_) => Err(ParseError::DigestUnsupported),
    }
}

// ── Conversions from/to strings ────────────────────────────────────────────

impl TryFrom<&str> for OciReference {
    type Error = ParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(ParseError::NameEmpty);
        }
        // A bare ':' or '@' prefix has no name component.
        if s.starts_with(':') || s.starts_with('@') {
            return Err(ParseError::ReferenceInvalidFormat);
        }

        // 1. Peel off the digest (`@<algo>:<hex>`).
        let (name_and_tag, digest) = match s.split_once('@') {
            Some((n, d)) => (n, Some(d.to_owned())),
            None => (s, None),
        };

        // 2. Peel off the tag.
        let (name, tag) = split_name_tag(name_and_tag);
        let mut tag = tag.map(str::to_owned);

        // 3. Normalise registry / repository.
        let (registry, repository) = split_domain(name);

        // 4. Default tag.
        if tag.is_none() && digest.is_none() {
            tag = Some(DEFAULT_TAG.to_owned());
        }

        // 5. Length check (repository only, mirrors the oci-spec behaviour).
        if repository.len() > NAME_TOTAL_LENGTH_MAX {
            return Err(ParseError::NameTooLong);
        }

        // 6. Character validation.
        validate_repository(&repository)?;
        if let Some(ref d) = digest {
            validate_digest(d)?;
        }

        let reference = match (tag, digest) {
            (Some(t), Some(d)) => Reference::with_tag_and_digest(registry, repository, t, d),
            (Some(t), None) => Reference::with_tag(registry, repository, t),
            (None, Some(d)) => Reference::with_digest(registry, repository, d),
            (None, None) => unreachable!("tag or digest is always set"),
        };
        Ok(OciReference(reference))
    }
}

impl FromStr for OciReference {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        OciReference::try_from(s)
    }
}

impl TryFrom<String> for OciReference {
    type Error = ParseError;
    fn try_from(string: String) -> Result<Self, Self::Error> {
        string.as_str().try_into()
    }
}

impl fmt::Display for OciReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        Ok(())
    }
}

impl From<OciReference> for String {
    fn from(reference: OciReference) -> Self {
        reference.0.whole()
    }
}

// ── Conversions to/from Reference ──────────────────────────────────────────

impl From<Reference> for OciReference {
    fn from(reference: Reference) -> Self {
        OciReference(reference)
    }
}

impl From<OciReference> for Reference {
    fn from(r: OciReference) -> Self {
        r.0
    }
}

/// Running all the tests from the current version of the `oci-spec` crate against
/// `OciReference` to verify that it behaves the same as `oci-spec`'s regex-based
/// `Reference` implementation.
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
            let reference = OciReference::try_from(input).expect("could not parse reference");
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
            // FIXME: should really pass a ParseError::NameContainsUppercase, but "invalid format" is good enough for now.
            case("Uppercase:tag", ParseError::ReferenceInvalidFormat),
            // FIXME: "Uppercase" is incorrectly handled as a domain-name here, and therefore passes.
            // https://github.com/docker/distribution/blob/master/reference/reference_test.go#L104-L109
            // case("Uppercase/lowercase:tag", ParseError::NameContainsUppercase),
            // FIXME: should really pass a ParseError::NameContainsUppercase, but "invalid format" is good enough for now.
            case("test:5000/Uppercase/lowercase:tag", ParseError::ReferenceInvalidFormat),
            case("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", ParseError::NameTooLong),
            case("aa/asdf$$^/aa", ParseError::ReferenceInvalidFormat)
        )]
        fn parse_bad_reference(input: &str, err: ParseError) {
            assert_eq!(OciReference::try_from(input).unwrap_err(), err)
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
            let mut reference = OciReference::try_from(input).expect("could not parse reference");
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
            let reference = OciReference::with_tag_and_digest(
                registry.to_string(),
                repository.to_string(),
                tag.to_string(),
                digest.to_string(),
            );
            assert_eq!(expected, reference.to_string());
        }
    }
}
