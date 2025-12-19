use crate::tools::test::TestResult;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Serialize)]
struct GraphQLRequest {
    query: String,
}

/// Executes a single NRQL query against the New Relic GraphQL API.
///
/// This function sends a GraphQL query to execute NRQL, check that
/// results are not empty and return its results.
///
/// # Arguments
///
/// * `api_endpoint` - The New Relic API endpoint (e.g., "https://staging-api.newrelic.com")
/// * `api_key` - New Relic API key for authentication
/// * `account_id` - New Relic account ID
/// * `nrql_query` - The NRQL query to execute
///
/// # Returns
///
/// * `Ok(Vec<Value>)` - The NRQL query results on success
/// * `Err` - Error if the query fails, returns errors, or has no results
pub fn check_query_results_are_not_empty(
    api_endpoint: &str,
    api_key: &str,
    account_id: u64,
    nrql_query: &str,
) -> TestResult<Vec<Value>> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let url = format!("{}/graphql", api_endpoint);
    let graphql_query = format!(
        r#"{{
  actor {{
    account(id: {account_id}) {{
      nrql(query: "{nrql_query}") {{
        results
      }}
    }}
  }}
}}"#
    );

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("API-Key", api_key)
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
        return Ok(results.clone());
    }

    Err("NRQL query returned no results".into())
}
