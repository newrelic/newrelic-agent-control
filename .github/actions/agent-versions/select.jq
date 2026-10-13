# Arg: $target ("HOST/LINUX", "HOST/WINDOWS" or "KUBERNETES"), $agents (agents.json).
# Input: the resolved raw data. Agents without an enabled version on the target are skipped.
def semver: split(".") | map(tonumber? // 0);
def stable: test("^[0-9]+\\.[0-9]+\\.[0-9]+$");
def alive: .enabled and (.eolDate == null or .eolDate > (now | todate));

. as $data
| [$agents[] as $agent
   | [($data.targets[$target] // [])[]
      | select(.agentType == $agent.imAgentType and (.agentVersion | stable) and alive)]
   | sort_by(.agentVersion | semver) | last
   | select(. != null)
   | ([.configurationDefinitions[] | select(.configurationType == "agent-type") | .configurationVersion]
       | sort_by(semver) | last) as $agentTypeVersion
   | if $agentTypeVersion == null then error("\($agent.imAgentType) \(.agentVersion) on \($target) has no agent-type configuration") else . end
   | {name: $agent.name, packageVersion: .agentVersion, agentTypeVersion: $agentTypeVersion}]
