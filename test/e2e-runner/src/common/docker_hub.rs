use crate::common::test::TestResult;
use semver::Version;
use serde::Deserialize;
use tracing::info;

const DOCKER_HUB_API: &str = "https://hub.docker.com/v2/repositories";

#[derive(Deserialize)]
struct TagsResponse {
    results: Vec<TagEntry>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct TagEntry {
    name: String,
}

pub fn latest_published_ac_tag() -> TestResult<String> {
    Ok(published_ac_tags(1)?.remove(0))
}

/// Top `count` published AC semver tags, descending (index 0 = latest).
pub fn published_ac_tags(count: usize) -> TestResult<Vec<String>> {
    highest_semver_tags("newrelic/agent-control-artifacts", count)
}

const AGENT_TYPES_REPOSITORY: &str = "newrelic/agent-control-agent-types";
const AGENT_TYPE_NAMESPACE: &str = "newrelic";

#[cfg(target_family = "unix")]
const AGENT_TYPE_TAG_PLATFORM: &str = "host-linux";
#[cfg(target_family = "windows")]
const AGENT_TYPE_TAG_PLATFORM: &str = "host-windows";

/// Agent type id (`newrelic/<name>:<version>`) with the highest version published in the
/// agent types repository for the platform the runner is built for. Panics if none is found.
pub fn latest_agent_type_id(name: &str) -> String {
    let version = latest_agent_type_version(name)
        .unwrap_or_else(|err| panic!("resolving latest agent type '{name}': {err}"));
    let agent_type_id = format!("{AGENT_TYPE_NAMESPACE}/{name}:{version}");
    info!(agent_type_id, "Resolved latest published agent type");
    agent_type_id
}

fn latest_agent_type_version(name: &str) -> TestResult<Version> {
    let tag_prefix = format!("{AGENT_TYPE_TAG_PLATFORM}-{name}-");
    let mut next_url = Some(format!(
        "{DOCKER_HUB_API}/{AGENT_TYPES_REPOSITORY}/tags/?name={tag_prefix}&page_size=100"
    ));
    let mut tags = Vec::new();
    while let Some(url) = next_url {
        let response: TagsResponse = reqwest::blocking::get(&url)?.error_for_status()?.json()?;
        tags.extend(response.results.into_iter().map(|entry| entry.name));
        next_url = response.next;
    }

    highest_version_with_prefix(tags, &tag_prefix)
        .ok_or_else(|| format!("no '{tag_prefix}<semver>' tag in {AGENT_TYPES_REPOSITORY}").into())
}

/// Highest semver among tags that are exactly `prefix` followed by a version. Anything else,
/// such as a longer agent type name sharing the prefix or a signature tag, is ignored.
fn highest_version_with_prefix(
    tags: impl IntoIterator<Item = String>,
    prefix: &str,
) -> Option<Version> {
    tags.into_iter()
        .filter_map(|tag| Version::parse(tag.strip_prefix(prefix)?).ok())
        .max()
}

/// Top `count` semver tags among the latest 100 tags in a Docker Hub repository, descending.
fn highest_semver_tags(repository: &str, count: usize) -> TestResult<Vec<String>> {
    let url = format!("{DOCKER_HUB_API}/{repository}/tags/?page_size=100");
    let response: TagsResponse = reqwest::blocking::get(&url)?.error_for_status()?.json()?;

    let mut versions: Vec<Version> = response
        .results
        .into_iter()
        .filter_map(|entry| Version::parse(&entry.name).ok())
        .collect();
    versions.sort_unstable_by(|a, b| b.cmp(a));
    versions.truncate(count);

    if versions.len() < count {
        return Err(format!(
            "only found {} semver tag(s) in {repository}, need {count}",
            versions.len()
        )
        .into());
    }
    Ok(versions.into_iter().map(|v| v.to_string()).collect())
}
