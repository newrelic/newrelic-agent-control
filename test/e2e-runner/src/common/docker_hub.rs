use crate::common::test::TestResult;
use semver::Version;
use serde::Deserialize;

const DOCKER_HUB_API: &str = "https://hub.docker.com/v2/repositories";

#[derive(Deserialize)]
struct TagsResponse {
    results: Vec<TagEntry>,
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
