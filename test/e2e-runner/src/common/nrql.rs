use crate::common::InstallationArgs;
use crate::common::test::TestResult;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;
use std::fmt::Display;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Serialize)]
struct GraphQLRequest {
    query: String,
}

const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default, Debug, Copy, Clone, PartialEq, clap::ValueEnum)]
pub enum Region {
    #[default]
    US,
    EU,
    JP,
    #[value(alias = "stg")]
    Staging,
}

impl Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let region_str = match self {
            Self::US => "US",
            Self::EU => "EU",
            Self::JP => "JP",
            Self::Staging => "STAGING",
        };
        write!(f, "{region_str}")
    }
}

impl Region {
    fn as_api_endpoint(&self) -> &str {
        match self {
            Region::US => "https://api.newrelic.com",
            Region::EU => "https://api.eu.newrelic.com",
            Region::JP => "https://api.jp.newrelic.com",
            Region::Staging => "https://staging-api.newrelic.com",
        }
    }
}

/// Executes a single NRQL query against the New Relic GraphQL API.
///
/// This function sends a GraphQL query to execute NRQL, check that
/// results are not empty.
///
/// # Arguments
///
/// * `args` - Struct defining all required parameters: `region`, `api-key`, ...
/// * `nrql_query` - The NRQL query to execute
///
/// # Returns
///
/// * `Ok(())` - The NRQL query results on success
/// * `Err` - Error if the query fails, returns errors, or has no results
pub fn check_query_results_are_not_empty(
    install_args: &InstallationArgs,
    nrql_query: &str,
) -> TestResult<Vec<Value>> {
    check_query_results(install_args, nrql_query, |r| !r.is_empty())
}

/// Executes a single NRQL query against the New Relic GraphQL API.
///
/// This function sends a GraphQL query to execute NRQL, check that
/// results satisfy the provided predicate.
///
/// # Arguments
///
/// * `args` - Struct defining all required parameters: `region`, `api-key`, ...
/// * `nrql_query` - The NRQL query to execute
/// * `predicate` - The predicate to satisfy
///
/// # Returns
///
/// * `Ok(())` - The NRQL query results on success
/// * `Err` - Error if the query fails, returns errors or does not satisefy the predicate.
pub fn check_query_results(
    install_args: &InstallationArgs,
    nrql_query: &str,
    predicate: impl FnOnce(&Vec<Value>) -> bool,
) -> TestResult<Vec<Value>> {
    let client = Client::builder().timeout(CLIENT_TIMEOUT).build()?;
    check_query_results_with_client(install_args, nrql_query, client, predicate)
}

/// Helper to execute [check_query_results_are_not_empty] with custom setup. Eg: setting up proxy.
fn check_query_results_with_client(
    install_args: &InstallationArgs,
    nrql_query: &str,
    client: Client,
    predicate: impl FnOnce(&Vec<Value>) -> bool,
) -> TestResult<Vec<Value>> {
    let api_endpoint = install_args.nr_region.as_api_endpoint();
    let url = format!("{}/graphql", api_endpoint);
    let graphql_query = format!(
        r#"{{
  actor {{
    account(id: {}) {{
      nrql(query: "{}") {{
        results
      }}
    }}
  }}
}}"#,
        install_args.nr_account_id, nrql_query,
    );

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("API-Key", &install_args.nr_api_key)
        .json(&GraphQLRequest {
            query: graphql_query,
        })
        .send()?;

    if !response.status().is_success() {
        return Err(format!("HTTP request failed: {}", response.status()).into());
    }

    let response_json: Value = response.json()?;

    // Check for GraphQL errors
    if let Some(errors) = response_json.get("errors")
        && let Some(error_array) = errors.as_array()
    {
        let error_messages: Vec<String> = error_array
            .iter()
            .filter_map(|e| e.get("message")?.as_str())
            .map(String::from)
            .collect();
        return Err(format!("GraphQL errors: {}", error_messages.join(", ")).into());
    }

    // Extract results from the response
    let result = response_json
        .get("data")
        .and_then(|d| d.get("actor"))
        .and_then(|a| a.get("account"))
        .and_then(|a| a.get("nrql"))
        .and_then(|n| n.get("results"))
        .and_then(|r| r.as_array());

    match result {
        Some(results) if predicate(results) => {
            info!(
                query = nrql_query,
                "The NRQL query returned results matching the predicate"
            );
            Ok(results.clone())
        }
        Some(results) => Err(format!("NRQL query '{nrql_query}' returned data, but it did not match the provided predicate.\n\tResults: {results:?}").into()),
        None => Err(format!("NRQL query '{nrql_query}' returned no results").into()),
  }
}
