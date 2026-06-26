use crate::common::oci::{OciRegistry, PushedPackage, push_hook_package};
use crate::common::on_drop::CleanUp;
use crate::common::runtime::tokio_runtime;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData, config};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::service::{STATUS_RUNNING, restart_service_and_wait};
use fake_opamp_server::FakeServer;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tracing::info;

const AGENT_ID: &str = "posthook-agent";
const AGENT_TYPE: &str = "newrelic/com.newrelic.posthook_e2e:0.1.0";
const AGENT_TYPE_FILE: &str = "/etc/newrelic-agent-control/dynamic-agent-types/posthook_e2e.yaml";

const SUPERVISOR_KEY_ATTR: &str = "supervisor.key";
const HOOK_ERROR_MSG: &str = "e2e-posthook-failed-on-purpose";

// Installs Agent Control with a sub-agent whose OCI package defines a successful
// `post_download_hook`, and verifies (via filesystem markers) that the hook runs and the agent
// starts afterwards.
pub fn test_post_download_hook_success(args: InstallationArgs) {
    info!("Starting post-download-hook success scenario");

    let registry = OciRegistry::start();

    let (hook_marker, agent_marker) = markers();

    // The hook succeeds; the agent then starts and writes its own marker.
    let hook_script = format!("#!/bin/bash\ntouch {hook_marker}\nexit 0\n");
    let agent_script = format!("#!/bin/bash\ntouch {agent_marker}\nexec sleep 3600\n");
    let pushed = push_hook_package(&hook_script, &agent_script);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let _clean_up = CleanUp::new(tear_down_test);

    deploy_hook_agent(args, &registry, &pushed, &mut opamp_server);

    info!("Verifying the post-download hook ran (hook marker created)");
    retry_panic(
        60,
        Duration::from_secs(2),
        "post-download hook marker",
        || marker_exists(&hook_marker),
    );

    info!("Verifying the agent started after the hook (agent marker created)");
    retry_panic(60, Duration::from_secs(2), "agent started marker", || {
        marker_exists(&agent_marker)
    });

    info!("Verifying Agent Control reports the sub-agent as healthy via OpAMP");
    retry_panic(60, Duration::from_secs(2), "sub-agent healthy", || {
        let instance = opamp_server
            .find_agents_with_identifying_attr(SUPERVISOR_KEY_ATTR, AGENT_ID)
            .into_iter()
            .next()
            .ok_or("sub-agent has not connected to OpAMP yet")?;
        let health = opamp_server
            .get_health_status(instance)
            .ok_or("sub-agent has not reported health yet")?;
        if health.healthy {
            Ok(())
        } else {
            Err(format!("sub-agent not healthy yet: {}", health.last_error).into())
        }
    });

    info!("Post-download-hook success scenario completed successfully");
}

// Installs Agent Control with a sub-agent whose OCI package defines a failing
// `post_download_hook`, and verifies that the hook runs but the agent is never started because
// the hook returns a non-zero exit code.
pub fn test_post_download_hook_failure(args: InstallationArgs) {
    info!("Starting post-download-hook failure scenario");

    let registry = OciRegistry::start();

    let (hook_marker, agent_marker) = markers();

    // The hook writes its marker, prints an error to stderr and then fails; the agent must never start.
    let hook_script =
        format!("#!/bin/bash\ntouch {hook_marker}\necho '{HOOK_ERROR_MSG}' >&2\nexit 1\n");
    let agent_script = format!("#!/bin/bash\ntouch {agent_marker}\nexec sleep 3600\n");
    let pushed = push_hook_package(&hook_script, &agent_script);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let _clean_up = CleanUp::new(tear_down_test);

    deploy_hook_agent(args, &registry, &pushed, &mut opamp_server);

    info!("Verifying the post-download hook ran (hook marker created)");
    retry_panic(
        60,
        Duration::from_secs(2),
        "post-download hook marker",
        || marker_exists(&hook_marker),
    );

    info!(
        "Verifying Agent Control reports the sub-agent as unhealthy with the hook error via OpAMP"
    );
    retry_panic(
        60,
        Duration::from_secs(2),
        "sub-agent unhealthy with hook error",
        || {
            let instance = opamp_server
                .find_agents_with_identifying_attr(SUPERVISOR_KEY_ATTR, AGENT_ID)
                .into_iter()
                .next()
                .ok_or("sub-agent has not connected to OpAMP yet")?;
            let health = opamp_server
                .get_health_status(instance)
                .ok_or("sub-agent has not reported health yet")?;
            if health.healthy {
                return Err("sub-agent still reports healthy".into());
            }
            if !health.last_error.contains(HOOK_ERROR_MSG) {
                return Err(format!(
                    "sub-agent unhealthy but last_error does not contain the hook stderr; got: {}",
                    health.last_error
                )
                .into());
            }
            Ok(())
        },
    );

    assert!(
        !Path::new(&agent_marker).exists(),
        "agent must not start when the post-download hook fails, but '{agent_marker}' was created"
    );

    info!("Post-download-hook failure scenario completed successfully");
}

// Installs Agent Control, registers the post-download-hook agent type, points Agent Control at the
// local registry and fake OpAMP server, and applies the sub-agent via a remote config.
fn deploy_hook_agent(
    args: InstallationArgs,
    registry: &OciRegistry,
    pushed: &PushedPackage,
    opamp_server: &mut FakeServer,
) {
    install_agent_control_from_recipe(&RecipeData {
        args,
        ..Default::default()
    });

    write_agent_type(&pushed.jwks_url);

    let version = pushed.reference.tag().unwrap();

    let ac_config = format!(
        r#"
agents: {{}}
fleet_control:
  endpoint: {}
  signature_validation:
    public_key_server_url: {}
oci:
  registry: {}
log:
  file:
    enabled: true
  level: debug
"#,
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        registry.url(),
    );
    config::update_config(linux::DEFAULT_AC_CONFIG_PATH, &ac_config);

    config::write_agent_local_config(
        &linux::local_config_path(AGENT_ID),
        format!("package_version: '{version}'\n"),
    );

    restart_service_and_wait(linux::SERVICE_NAME, STATUS_RUNNING);
    info!("Agent Control restarted with local registry and fake OpAMP configuration");

    let instance_id = retry_panic(
        20,
        Duration::from_secs(2),
        "AC connecting to OpAMP server",
        || {
            opamp_server
                .find_agent_control_instance()
                .map_err(|e| e.into())
        },
    );

    let agents = format!(
        r#"
agents:
  {AGENT_ID}:
    agent_type: "{AGENT_TYPE}"
"#
    );
    opamp_server.set_config_response(instance_id, agents);
    info!("Sent remote config deploying the post-download-hook sub-agent");
}

// Writes the custom agent type definition to the host's dynamic-agent-types directory.
// The package wires `hook.sh` as the `post_download_hook` (run with its working directory set to
// the package dir) and `agent.sh` as the agent executable.
fn write_agent_type(jwks_url: &str) {
    let yaml = format!(
        r#"
namespace: newrelic
name: com.newrelic.posthook_e2e
version: 0.1.0
platform: host
operating_system: linux
protocol_version: "1.0"
variables:
  package_version:
    description: "OCI package version to download"
    type: string
    required: false
    default: latest
deployment:
  packages:
    test-package:
      type: tar
      download:
        oci:
          repository: test
          version: ${{nr-var:package_version}}
          public_key_url: {jwks_url}
      post_download_hook:
        path: /bin/bash
        args:
          - hook.sh
  executables:
    - id: posthook-agent-exec
      path: /bin/bash
      args:
        - ${{nr-sub:packages.test-package.dir}}/agent.sh
"#
    );

    let path = Path::new(AGENT_TYPE_FILE);
    fs::create_dir_all(path.parent().unwrap())
        .expect("failed to create dynamic-agent-types directory");
    fs::write(path, yaml).expect("failed to write the post-download-hook agent type");
}

// Returns unique (hook, agent) marker paths for this run so stale files from previous runs on the
// same host can't produce false positives.
fn markers() -> (String, String) {
    let run_id = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    (
        format!("/tmp/ac-e2e-posthook-{run_id}.hook"),
        format!("/tmp/ac-e2e-posthook-{run_id}.agent"),
    )
}

fn marker_exists(path: &str) -> TestResult<()> {
    if Path::new(path).exists() {
        Ok(())
    } else {
        Err(format!("marker '{path}' not present yet").into())
    }
}
