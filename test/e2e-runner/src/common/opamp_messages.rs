use crate::common::test::TestResult;
use fake_opamp_server::{FakeServer, InstanceID};
use opamp_client::opamp::proto::AgentToServer;
use opamp_client::opamp::proto::any_value::Value;

/// Non-identifying attribute Agent Control sets to `"dry-run"` while running its self-update
/// pre-flight verify check. Kept in sync with `EXECUTION_MODE_ATTRIBUTE_KEY` in
/// `agent_control::defaults` on the Agent Control side.
const EXECUTION_MODE_ATTRIBUTE_KEY: &str = "execution.mode";

/// Returns whether the message's agent description carries the `execution.mode=dry-run`
/// non-identifying attribute, i.e. it was sent by a client running in dry-run/verify mode.
fn has_dry_run_execution_mode(message: &AgentToServer) -> bool {
    let Some(description) = message.agent_description.as_ref() else {
        return false;
    };

    description.non_identifying_attributes.iter().any(|kv| {
        kv.key == EXECUTION_MODE_ATTRIBUTE_KEY
            && matches!(
                kv.value.as_ref().and_then(|v| v.value.as_ref()),
                Some(Value::StringValue(val)) if val == "dry-run"
            )
    })
}

/// Checks that the given agent has sent, among all its received messages, at least one
/// dry-run "starting" message (no `agent_disconnect`) and at least one dry-run "shutdown"
/// message (`agent_disconnect` present).
///
/// During self-update, Agent Control spawns the newly downloaded binary as a pre-flight
/// `verify` subprocess before self-replacing; that subprocess reports under the same instance
/// ID as the currently-running service. Classifying by the dry-run attribute and the disconnect
/// flag (rather than message order or count) keeps this correct regardless of how the verify
/// subprocess's messages interleave with the running service's own messages.
pub fn check_dry_run_start_and_shutdown_messages(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
) -> TestResult<()> {
    let dry_run_messages: Vec<AgentToServer> = opamp_server
        .get_messages(instance_id.clone())
        .into_iter()
        .filter(has_dry_run_execution_mode)
        .collect();

    if !dry_run_messages
        .iter()
        .any(|message| message.agent_disconnect.is_none())
    {
        return Err("no dry-run starting message (without agent_disconnect) found".into());
    }

    if !dry_run_messages
        .iter()
        .any(|message| message.agent_disconnect.is_some())
    {
        return Err("no dry-run shutdown message (with agent_disconnect) found".into());
    }

    Ok(())
}
