use reqwest::blocking::Client;
use std::time::Duration;

#[test]
#[ignore = "needs oci registry"]
fn test_oci_registry_is_running_with_oci_registry() {
    let registry_url = "http://localhost:5000".to_string();

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create HTTP client");

    let response = client
        .get(format!("{}/v2/", registry_url))
        .send()
        .expect("Failed to connect to OCI registry");

    assert!(
        response.status().is_success(),
        "OCI registry should return 200 for /v2/ endpoint, got: {}",
        response.status()
    );
}
