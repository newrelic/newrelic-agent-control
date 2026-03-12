use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::health::check_latest_health_status;
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::tools::config::create_agent_control_config;
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use memory_stats::memory_stats;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_memory_on_agent_substitution_and_version_update() {
    // 5 MiB (Leak Protection)
    let allowed_growth = 5 * 1024 * 1024;
    // 150 MiB (Bloat Protection) 120 MiB is the actual max happening on first iteration
    let max_memory_limit = 150 * 1024 * 1024;

    let local_dir = tempdir().expect("failed to create local temp dir");
    let agent_id = "nr-sleep-agent";

    let packages_config = format!(
        r#"
{agent_id}:
  type: tar
  download:
    oci:
      registry: non-existent:5000
      repository: test
      version: ${{nr-var:fake_variable}}
"#
    );

    // Add custom agent_type to registry
    let sleep_agent_type = CustomAgentType::default()
        .with_executables(Some(
            r#"[
                {"id": "trap-term-sleep", "path": "sh", "args": ["tests/on_host/data/sleep_60.sh"]},
            ]"#,
        ))
        .with_packages(Some(&packages_config))
        .build(local_dir.path().to_path_buf());

    let mut opamp_server = FakeServer::start_new();
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        "{}".to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let mut memory_samples = Vec::new();

    for version in 1..8 {
        // Create a new sub-agent every time to create a new thread
        let new_agent_id = format!("{}-{}", agent_id, version);

        let agent_a = format!(
            r#"
        agents:
          {new_agent_id}:
            agent_type: "{sleep_agent_type}"
        "#
        );

        opamp_server.set_config_response(ac_instance_id.clone(), agent_a);

        let sleep_instance_id = get_instance_id(
            &AgentID::try_from(new_agent_id).unwrap(),
            base_paths.clone(),
        );

        // A new version to call the template and compute the oci reference
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let sleep_agent_cfg = format!("fake_variable: '{timestamp}.{timestamp}.{timestamp}'");
        opamp_server.set_config_response(sleep_instance_id.clone(), sleep_agent_cfg);

        retry(60, Duration::from_secs(1), || {
            check_latest_health_status(&opamp_server, &sleep_instance_id, |status| {
                !status.healthy && status.last_error.contains("failure installing package")
            })
        });

        // Force failure if memory cannot be read
        let usage = memory_stats().expect("Failed to read memory stats from the OS");
        let current_mem = usage.physical_mem;

        assert!(
            current_mem <= max_memory_limit,
            "Memory bloat detected! Current usage ({} MiB) exceeds the limit ({} MiB)",
            current_mem / 1024 / 1024,
            max_memory_limit / 1024 / 1024
        );

        memory_samples.push(current_mem);
        println!("Iteration {}: {} MiB", version, current_mem / 1024 / 1024);
    }

    let first_stable = memory_samples[0]; // Baseline
    let last = *memory_samples.last().unwrap();

    println!(
        "Total Growth: {} bytes",
        last as i128 - first_stable as i128
    );

    assert!(
        last <= first_stable + allowed_growth,
        "Memory leak detected! Initial: {} B, Final: {} B",
        first_stable,
        last
    );
}
