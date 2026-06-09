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
    latest_semver_tag("newrelic/agent-control-artifacts")
}

/// Fetches the highest semver tag published for the latest 100 tags in a Docker Hub repository.
fn latest_semver_tag(repository: &str) -> TestResult<String> {
    let url = format!("{DOCKER_HUB_API}/{repository}/tags/?page_size=100");
    let response: TagsResponse = reqwest::blocking::get(&url)?.error_for_status()?.json()?;

    response
        .results
        .into_iter()
        .filter_map(|entry| Version::parse(&entry.name).ok())
        .max()
        .map(|version| version.to_string())
        .ok_or_else(|| format!("no semver tag found in {repository}").into())
}
