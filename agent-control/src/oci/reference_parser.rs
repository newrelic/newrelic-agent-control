use std::str::FromStr;

use oci_client::{ParseError, Reference};

use crate::{
    agent_control::config::Registry,
    agent_type::runtime_config::on_host::package::rendered::{Repository, Version},
};

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

impl TryFrom<(&str, &str, &str)> for ReferenceParser {
    type Error = ParseError;

    fn try_from((registry, repository, version): (&str, &str, &str)) -> Result<Self, Self::Error> {
        let registry =
            Registry::from_str(registry).map_err(|_| ParseError::ReferenceInvalidFormat)?;
        let repository =
            Repository::from_str(repository).map_err(|_| ParseError::ReferenceInvalidFormat)?;
        let version = Version::from_str(version).map_err(|_| ParseError::ReferenceInvalidFormat)?;

        Ok(Self::from((&registry, &repository, &version)))
    }
}

impl From<ReferenceParser> for Reference {
    fn from(value: ReferenceParser) -> Self {
        value.0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    mod parse {
        use super::*;
        use rstest::rstest;

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
    }
}
