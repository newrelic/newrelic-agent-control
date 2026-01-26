use crate::common::Args;
use crate::common::test::TestResult;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Serialize)]
struct GraphQLRequest {
    query: String,
}

const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
enum Region {
    US,
    EU,
    Staging,
}

impl TryFrom<&str> for Region {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_ref() {
            "us" => Ok(Self::US),
            "eu" => Ok(Self::EU),
            "staging" => Ok(Self::Staging),
            _ => Err(format!("Invalid region '{value}'")),
        }
    }
}

impl Region {
    fn api_endpoint(self) -> String {
        match self {
            Region::US => "https://api.newrelic.com".to_string(),
            Region::EU => "https://api.eu.newrelic.com".to_string(),
            Region::Staging => "https://staging-api.newrelic.com".to_string(),
        }
    }
}

/// Executes a single NRQL query against the New Relic GraphQL API.
///
/// This function sends a GraphQL query to execute NRQL, check that
/// results are not empty and return its results.
///
/// # Arguments
///
/// * `args` - Struct defining all required parameters: `region`, `api-key`, ...
/// * `nrql_query` - The NRQL query to execute
///
/// # Returns
///
/// * `Ok(Vec<Value>)` - The NRQL query results on success
/// * `Err` - Error if the query fails, returns errors, or has no results
pub fn check_query_results_are_not_empty(
    install_args: &Args,
    nrql_query: &str,
) -> TestResult<Vec<Value>> {
    let client = Client::builder().timeout(CLIENT_TIMEOUT).build()?;
    check_query_results_are_not_empty_with_client(install_args, nrql_query, client)
}

/// Helper to execute [check_query_results_are_not_empty] with custom setup. Eg: setting up proxy.
fn check_query_results_are_not_empty_with_client(
    install_args: &Args,
    nrql_query: &str,
    client: Client,
) -> TestResult<Vec<Value>> {
    let api_endpoint = Region::try_from(install_args.nr_region.as_str())?.api_endpoint();
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
    if let Some(results) = response_json
        .get("data")
        .and_then(|d| d.get("actor"))
        .and_then(|a| a.get("account"))
        .and_then(|a| a.get("nrql"))
        .and_then(|n| n.get("results"))
        .and_then(|r| r.as_array())
        && !results.is_empty()
    {
        info!(
            query = nrql_query,
            "The NRQL query returned results as expected"
        );
        return Ok(results.clone());
    }

    Err(format!("NRQL query '{nrql_query}' returned no results").into())
}
