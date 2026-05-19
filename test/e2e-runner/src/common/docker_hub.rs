use crate::common::test::TestResult;
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
    latest_tag("newrelic/agent-control-artifacts")
}

/// Fetches the most recent tag published for a Docker Hub repository.
fn latest_tag(repository: &str) -> TestResult<String> {
    let url = format!("{DOCKER_HUB_API}/{repository}/tags/?page_size=100");
    let response: TagsResponse = reqwest::blocking::get(&url)?.error_for_status()?.json()?;

    // look for the first non signature tag.
    response
        .results
        .into_iter()
        .map(|entry| entry.name)
        .find(|name| !name.starts_with("sha"))
        .ok_or_else(|| format!("tag not found in {repository}").into())
}
