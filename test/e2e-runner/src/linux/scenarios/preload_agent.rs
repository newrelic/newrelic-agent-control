use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::file::write;
use crate::common::on_drop::CleanUp;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::{
    self,
    bash::exec_bash_command,
    install::{install_agent_control_from_recipe, tear_down_test},
};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info};
/// Directory where Agent Control loads dynamic (custom) agent type definitions.
const DYNAMIC_AGENT_TYPES_DIR: &str = "/etc/newrelic-agent-control/dynamic-agent-types";
/// World-readable path the preload `.so` is staged to by `install_preload.sh`.
/// /var/lib/newrelic-agent-control isn't traversable by non-root, so referencing the OCI-managed
/// copy directly from /etc/ld.so.preload breaks every dynamically linked binary for non-root users.
const STAGED_PRELOAD_SO: &str = "/usr/local/lib/newrelic-preload-agent.so";
/// Host directory the post download hook scripts are written to, so the agent type only needs to
/// reference them.
const HOOK_SCRIPTS_DIR: &str = "/opt/nr-e2e-preload-agent";
const INSTALL_SCRIPT: &str = include_str!("preload_agent/install.sh");
const INSTALL_PRELOAD_SCRIPT: &str = include_str!("preload_agent/install_preload.sh");
const INSTALL_JAVA_AGENT_SCRIPT: &str = include_str!("preload_agent/install_java_agent.sh");
/// Host directory the sample application is written to.
const SAMPLE_APP_DIR: &str = "/opt/nr-e2e-sample-app";
const SAMPLE_APP_SOURCE: &str = include_str!("preload_agent/SampleApp.java");
/// Directory under the one the preload library scans for fleet configs
/// (`/var/lib/auto-generated/agents.d`), holding the fleet config and the Java agent for the
/// sample app.
const JAVA_FLEET_DIR: &str = "/var/lib/auto-generated/agents.d/e2e-java";
/// Name the sample app reports and the fleet config applies to.
const SAMPLE_APP_NAME: &str = "e2e-sample-app";
/// Not a valid New Relic key, which is enough for the agent to start.
const DUMMY_LICENSE_KEY: &str = "0123456789abcdef0123456789abcdef01234567";
/// Env var that sets the preload library log level. Its logger writes to stderr.
const PRELOAD_LOG_ENV: &str = "NR_PRELOAD_LOG";
/// Debug log of the preload library for processes that are not an APM runtime (e.g. `echo`), which
/// shows it was loaded into the process and ran without instrumenting anything.
const PRELOAD_SKIPPED_LOG: &str = "not an instrumentation candidate";
/// Log of the preload library when it matched a fleet config and instrumented the process.
const PRELOAD_INJECTED_LOG: &str = "pre-load successful";
/// Log of the Java agent once it has started inside the JVM.
const JAVA_AGENT_STARTED_LOG: &str = "Premain startup complete";

/// Removes everything the scenario put on the host so it does not leak into later processes,
/// then runs the common test tear down.
fn tear_down_preload_test() {
    let cleanup = format!(
        "sed -i '\\|^{STAGED_PRELOAD_SO}$|d' /etc/ld.so.preload; \
         [ -s /etc/ld.so.preload ] || rm -f /etc/ld.so.preload; \
         rm -rf {STAGED_PRELOAD_SO} {HOOK_SCRIPTS_DIR} {SAMPLE_APP_DIR} {JAVA_FLEET_DIR}"
    );
    if let Err(err) = exec_bash_command(&cleanup) {
        debug!("Failed to remove the preload test files from the host: {err}");
    }
    tear_down_test();
}

/// Expected package installation directory for the preload agent.
fn preload_package_dir() -> String {
    format!(
        "{}/packages/nr-preload/stored_packages/preload-agent",
        linux::AGENT_CONTROL_DATA_DIR
    )
}

fn java_agent_jar() -> String {
    format!("{JAVA_FLEET_DIR}/java_apm_agent/newrelic.jar")
}

fn java_agent_config() -> String {
    format!("{JAVA_FLEET_DIR}/java_apm_agent/newrelic.yml")
}

fn java_agent_log() -> String {
    format!("{JAVA_FLEET_DIR}/java_apm_agent/logs/newrelic_agent.log")
}

pub fn test_installation_with_preload_agent(args: InstallationArgs) {
    let preload_version = args
        .preload_version
        .clone()
        .expect("--preload-version is required for this scenario");
    let java_agent_version = args
        .java_agent_version
        .clone()
        .expect("--java-agent-version is required for this scenario");
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_preload_test);
    install_agent_control_from_recipe(&recipe_data);
    // after the above is done inspect the directory structures to make sure everything is in there
    // particularly the AC local configuration
    let ac_local_config_path = Path::new(&linux::DEFAULT_AC_CONFIG_PATH);
    assert!(
        ac_local_config_path.exists(),
        "AC local config should exist"
    );
    debug!(
        "AC config file contents: {}",
        std::fs::read_to_string(ac_local_config_path).expect("failed to read AC config file")
    );
    let test_id = format!(
        "onhost-e2e-preload-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );
    let preload_agent_id = "nr-preload";

    info!("Installing the tools the post download hook and the sample application need");
    install_host_dependencies();
    info!("Writing post download hook scripts and the sample application");
    write_hook_scripts();
    write_sample_app();
    write_fleet_config(&java_agent_version);

    info!("Writing custom preload agent type definition");
    let custom_agent_type_path = format!("{DYNAMIC_AGENT_TYPES_DIR}/preload.yaml");
    let custom_agent_type = r#"namespace: newrelic
name: com.newrelic.preload
version: 0.1.0
platform: host
operating_system: linux
protocol_version: "1.0"
variables:
  oci:
    repository:
      description: "Package repository name"
      type: string
      required: false
      default: newrelic/preload-agent-artifacts
  version:
    description: "Agent version"
    type: string
    required: true
  java_agent:
    version:
      description: "Version of the Java APM agent to download from Maven Central"
      type: string
      required: true
    dir:
      description: "Directory where the preload library looks for the fleet config and the Java agent"
      type: string
      required: true
deployment:
  packages:
    preload-agent:
      download:
        oci:
          repository: ${nr-var:oci.repository}
          version: ${nr-var:version}
          public_key_url: https://publickeys.newrelic.com/g/agent-control-oci/global/nrpreloadagent/jwks.json
      post_download_hook:
        path: /bin/bash
        args:
          - __HOOK_SCRIPTS_DIR__/install.sh
          - ${nr-var:java_agent.version}
          - ${nr-var:java_agent.dir}
"#
    .replace("__HOOK_SCRIPTS_DIR__", HOOK_SCRIPTS_DIR);
    exec_bash_command(&format!("mkdir -p {DYNAMIC_AGENT_TYPES_DIR}"))
        .unwrap_or_else(|err| panic!("Failed to create dynamic agent types directory: {err}"));
    write(&custom_agent_type_path, &custom_agent_type);
    info!("Setup Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  {preload_agent_id}:
    agent_type: "newrelic/com.newrelic.preload:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );
    write_agent_local_config(
        &linux::local_config_path(preload_agent_id),
        format!(
            r#"
version: {preload_version}
java_agent:
  version: {java_agent_version}
  dir: {JAVA_FLEET_DIR}
"#
        ),
    );
    linux::service::restart_service(linux::SERVICE_NAME);
    info!("Waiting for preload OCI package to be installed and its post-download hook to apply");
    let package_dir = preload_package_dir();
    let retries = 60;
    retry_panic(
        retries,
        Duration::from_secs(10),
        "preload package installation assertion",
        || assert_preload_agent_installed(&package_dir),
    );
    info!("Running the sample application with the preload library");
    retry_panic(
        3,
        Duration::from_secs(5),
        "sample application instrumentation assertion",
        assert_sample_app_instrumented,
    );
    info!("Test completed successfully");
}

/// Installs `unzip` (used by the Java agent script) and a JDK that can run a single source file.
fn install_host_dependencies() {
    exec_bash_command(
        "DEBIAN_FRONTEND=noninteractive apt-get update -qq \
         && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq unzip default-jdk-headless",
    )
    .unwrap_or_else(|err| panic!("Failed to install the host dependencies: {err}"));
}

fn write_hook_scripts() {
    exec_bash_command(&format!("mkdir -p {HOOK_SCRIPTS_DIR}"))
        .unwrap_or_else(|err| panic!("Failed to create the hook scripts directory: {err}"));
    for (name, content) in [
        ("install.sh", INSTALL_SCRIPT),
        ("install_preload.sh", INSTALL_PRELOAD_SCRIPT),
        ("install_java_agent.sh", INSTALL_JAVA_AGENT_SCRIPT),
    ] {
        write(format!("{HOOK_SCRIPTS_DIR}/{name}"), content);
    }
}

fn write_sample_app() {
    exec_bash_command(&format!("mkdir -p {SAMPLE_APP_DIR}"))
        .unwrap_or_else(|err| panic!("Failed to create the sample app directory: {err}"));
    write(format!("{SAMPLE_APP_DIR}/SampleApp.java"), SAMPLE_APP_SOURCE);
}

/// Writes the fleet config Agent Control will generate for APM agents. The preload library finds
/// the Java agent next to it, in `java_apm_agent/`.
fn write_fleet_config(java_agent_version: &str) {
    exec_bash_command(&format!("mkdir -p {JAVA_FLEET_DIR}"))
        .unwrap_or_else(|err| panic!("Failed to create {JAVA_FLEET_DIR}: {err}"));
    write(
        format!("{JAVA_FLEET_DIR}/apm_fleet_config.yaml"),
        format!(
            r#"fleet_id: e2efleet
apm_language: java
agent_version: {java_agent_version}
application_names:
  - {SAMPLE_APP_NAME}
new_relic_license_key: {DUMMY_LICENSE_KEY}
"#
        ),
    );
}

fn assert_preload_agent_installed(package_dir: &str) -> TestResult<()> {
    let output = exec_bash_command(&format!("ls -d {package_dir}"))?;
    if output.contains("No such file") || output.contains("cannot access") {
        return Err(format!("Preload package directory not found yet at {package_dir}").into());
    }
    let listing = exec_bash_command(&format!("ls -la {package_dir}"))?;
    debug!("Package listing:\n{listing}");

    if !Path::new(STAGED_PRELOAD_SO).exists() {
        return Err(
            format!("Staged preload shared library not found yet at {STAGED_PRELOAD_SO}").into(),
        );
    }
    let ld_preload = std::fs::read_to_string("/etc/ld.so.preload")
        .map_err(|err| format!("Failed to read /etc/ld.so.preload: {err}"))?;
    if !ld_preload
        .lines()
        .any(|line| line.trim() == STAGED_PRELOAD_SO)
    {
        return Err(format!("{STAGED_PRELOAD_SO} not yet present in /etc/ld.so.preload").into());
    }
    for file in [java_agent_jar(), java_agent_config()] {
        if !Path::new(&file).exists() {
            return Err(format!("Java agent file not found yet at {file}").into());
        }
    }
    assert_preload_library_runs()
}

/// Extracts the stdout of a successful `exec_bash_command` result, which also echoes the command
/// text. Assertions must read this, or they could match the command itself.
fn stdout_of(result: &str) -> &str {
    let stdout = result
        .split_once("\nsuccess\nStdout: ")
        .map_or(result, |(_, rest)| rest);
    stdout
        .rsplit_once("\nStderr: ")
        .map_or(stdout, |(out, _)| out)
}

/// Checks the preload library gets loaded into new processes and that it ran, without requiring
/// it to instrument anything.
///
/// - Loaded: a fresh `grep` lists the library in its own `/proc/self/maps`, both with an explicit
///   `LD_PRELOAD` and with none (so only `/etc/ld.so.preload` can have loaded it).
/// - Ran: the library logs (enabled with `NR_PRELOAD_LOG`) that a process which is not an APM
///   runtime is not an instrumentation candidate.
fn assert_preload_library_runs() -> TestResult<()> {
    for (mode, ld_preload) in [
        (
            "explicit LD_PRELOAD",
            format!("LD_PRELOAD={STAGED_PRELOAD_SO} "),
        ),
        ("/etc/ld.so.preload", String::new()),
    ] {
        let out = exec_bash_command(&format!(
            "{ld_preload}grep -c {STAGED_PRELOAD_SO} /proc/self/maps || true"
        ))?;
        let mapped = stdout_of(&out).trim().parse::<u32>().unwrap_or(0);
        debug!("Preload library mappings through {mode}: {mapped}");
        if mapped == 0 {
            return Err(format!(
                "Preload library not loaded into a new process through {mode}: {out}"
            )
            .into());
        }
    }

    // The computed output can't be confused with the command text.
    let out = exec_bash_command(&format!(
        "{PRELOAD_LOG_ENV}=debug /bin/echo probe-$((20+22)) 2>&1"
    ))?;
    debug!("Probe through /etc/ld.so.preload: {out}");
    if !stdout_of(&out).contains("probe-42") {
        return Err(
            format!("Probed binary did not produce its output while preloaded: {out}").into(),
        );
    }
    if !stdout_of(&out).contains(PRELOAD_SKIPPED_LOG) {
        return Err(
            format!("Preload library did not log through /etc/ld.so.preload: {out}").into(),
        );
    }
    Ok(())
}

/// Runs the sample application as a JVM and checks the preload library instruments it with the
/// Java agent installed by the hook, and leaves it alone when it is not meant to be instrumented.
///
/// A `-javaagent` flag marks a JVM as one to be instrumented, so the real agent jar is used as
/// marker. The agent only warns about the second `-javaagent` the preload library adds.
fn assert_sample_app_instrumented() -> TestResult<()> {
    let jar = java_agent_jar();
    let app = format!("{SAMPLE_APP_DIR}/SampleApp.java");

    let instrumented = exec_bash_command(&format!(
        "{PRELOAD_LOG_ENV}=debug NEW_RELIC_APP_NAME={SAMPLE_APP_NAME} java -javaagent:{jar} {app} 2>&1"
    ))?;
    debug!("Instrumented sample application run: {instrumented}");
    let stdout = stdout_of(&instrumented);
    for expected in [
        PRELOAD_INJECTED_LOG,
        &format!("JAVA_TOOL_OPTIONS: -javaagent:{jar}"),
        &format!("NEW_RELIC_APP_NAME: {SAMPLE_APP_NAME}"),
        &format!("NEWRELIC_FILE: {}", java_agent_config()),
        "agent-attached: true",
    ] {
        if !stdout.contains(expected) {
            return Err(format!(
                "Instrumented sample application output misses '{expected}': {instrumented}"
            )
            .into());
        }
    }
    let agent_log = std::fs::read_to_string(java_agent_log())
        .map_err(|err| format!("Failed to read the Java agent log: {err}"))?;
    if !agent_log.contains(JAVA_AGENT_STARTED_LOG) {
        return Err(format!("Java agent did not start, its log is:\n{agent_log}").into());
    }

    // Not marked as to be instrumented (no `-javaagent`): nothing is injected.
    let not_marked = exec_bash_command(&format!(
        "NEW_RELIC_APP_NAME={SAMPLE_APP_NAME} java {app} 2>&1"
    ))?;
    debug!("Sample application run without -javaagent: {not_marked}");
    for expected in ["JAVA_TOOL_OPTIONS: null", "agent-attached: false"] {
        if !stdout_of(&not_marked).contains(expected) {
            return Err(format!(
                "Sample application without -javaagent output misses '{expected}': {not_marked}"
            )
            .into());
        }
    }

    // No fleet config applies to this application name: nothing is injected.
    let unknown_app = exec_bash_command(&format!(
        "{PRELOAD_LOG_ENV}=debug NEW_RELIC_APP_NAME=not-in-any-fleet-config java -javaagent:{jar} {app} 2>&1"
    ))?;
    debug!("Sample application run with an unknown name: {unknown_app}");
    if !stdout_of(&unknown_app).contains("JAVA_TOOL_OPTIONS: null") {
        return Err(format!(
            "Preload library instrumented an application with no fleet config: {unknown_app}"
        )
        .into());
    }
    Ok(())
}
