use std::{fmt, str::FromStr};

use thiserror::Error;

/// NAME_TOTAL_LENGTH_MAX is the maximum total number of characters in a repository name.
const NAME_TOTAL_LENGTH_MAX: usize = 255;

/// Reasons that parsing a string as a Reference can fail.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    /// Will be returned if digest is ill-formed
    #[error("invalid checksum digest format")]
    DigestInvalidFormat,
    /// Will be returned if digest does not have a correct length
    #[error("invalid checksum digest length")]
    DigestInvalidLength,
    /// Will be returned for an unknown digest algorithm
    #[error("unsupported digest algorithm")]
    DigestUnsupported,
    /// Will be returned for an uppercase character in repository name
    #[error("repository name must be lowercase")]
    NameContainsUppercase,
    /// Will be returned if a name is empty
    #[error("repository name must have at least one component")]
    NameEmpty,
    /// Will be returned if a name is too long
    #[error("repository name must not be more than {NAME_TOTAL_LENGTH_MAX} characters")]
    NameTooLong,
    /// Will be returned if a reference is ill-formed
    #[error("invalid reference format")]
    ReferenceInvalidFormat,
    /// Will be returned if a tag is ill-formed
    #[error("invalid tag format")]
    TagInvalidFormat,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Reference {
    registry: String,
    // #[serde(skip_serializing_if = "Option::is_none")]
    mirror_registry: Option<String>,
    repository: String,
    // #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<String>,
}

impl Reference {
    /// Create a new instance of [`Reference`] with a registry, repository, tag and digest.
    ///
    /// This is useful when you need to reference an image by both its semantic version (tag)
    /// and its content-addressable digest for immutability.
    ///
    /// # Examples
    ///
    /// ```
    /// use oci_spec::distribution::Reference;
    ///
    /// let reference = Reference::with_tag_and_digest(
    ///     "docker.io".to_string(),
    ///     "library/nginx".to_string(),
    ///     "1.21".to_string(),
    ///     "sha256:abc123...".to_string(),
    /// );
    /// ```
    pub fn with_tag_and_digest(
        registry: String,
        repository: String,
        tag: String,
        digest: String,
    ) -> Self {
        Self {
            registry,
            mirror_registry: None,
            repository,
            tag: Some(tag),
            digest: Some(digest),
        }
    }
    /// Set a pull mirror registry for this reference.
    ///
    /// The mirror registry will be used to resolve the image, the original registry
    /// is available via the [`Reference::namespace`] function.
    ///
    /// The original registry will be sent with the `ns` query parameter to the mirror registry.
    /// The `ns` query parameter is currently not part of the stable OCI Distribution Spec yet,
    /// but is being discussed to be added and is already used by some other implementations
    /// (for example containerd). So be aware that this feature might not work with all registries.
    ///
    /// Since this is not part of the stable OCI Distribution Spec yet, this feature is exempt from
    /// semver backwards compatibility guarantees and might change in the future.
    #[doc(hidden)]
    pub fn set_mirror_registry(&mut self, registry: String) {
        self.mirror_registry = Some(registry);
    }

    /// Resolve the registry address of a given `Reference`.
    ///
    /// Some registries, such as docker.io, uses a different address for the actual
    /// registry. This function implements such redirection.
    ///
    /// If a mirror registry is set, it will be used instead of the original registry.
    pub fn resolve_registry(&self) -> &str {
        match (self.registry(), self.mirror_registry.as_deref()) {
            (_, Some(mirror_registry)) => mirror_registry,
            ("docker.io", None) => "index.docker.io",
            (registry, None) => registry,
        }
    }

    /// Returns the name of the registry.
    pub fn registry(&self) -> &str {
        &self.registry
    }

    /// Returns the name of the repository.
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// Returns the object's tag, if present.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// Returns the object's digest, if present.
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }

    /// Returns the original registry when pulled via a mirror.
    ///
    /// Since this is not part of the stable OCI Distribution Spec yet, this feature is exempt from
    /// semver backwards compatibility guarantees and might change in the future.
    #[doc(hidden)]
    pub fn namespace(&self) -> Option<&str> {
        if self.mirror_registry.is_some() {
            Some(self.registry())
        } else {
            None
        }
    }

    /// Returns the whole reference.
    pub fn whole(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut not_empty = false;
        if !self.registry().is_empty() {
            write!(f, "{}", self.registry())?;
            not_empty = true;
        }
        if !self.repository().is_empty() {
            if not_empty {
                write!(f, "/")?;
            }
            write!(f, "{}", self.repository())?;
            not_empty = true;
        }
        if let Some(t) = self.tag() {
            if not_empty {
                write!(f, ":")?;
            }
            write!(f, "{t}")?;
            not_empty = true;
        }
        if let Some(d) = self.digest() {
            if not_empty {
                write!(f, "@")?;
            }
            write!(f, "{d}")?;
        }
        Ok(())
    }
}

impl FromStr for Reference {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Reference::try_from(s)
    }
}

impl TryFrom<&str> for Reference {
    type Error = ParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(ParseError::NameEmpty);
        }
        todo!()
    }
}

impl TryFrom<String> for Reference {
    type Error = ParseError;
    fn try_from(string: String) -> Result<Self, Self::Error> {
        TryFrom::try_from(string.as_str())
    }
}

impl From<Reference> for String {
    fn from(reference: Reference) -> Self {
        reference.whole()
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
            let reference = Reference::try_from(input).expect("could not parse reference");
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
            assert_eq!(Reference::try_from(input).unwrap_err(), err)
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
            let mut reference = Reference::try_from(input).expect("could not parse reference");
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
