# Arg: $target ("HOST/LINUX", "HOST/WINDOWS" or "KUBERNETES").
# Input: the resolved raw data. Agent types without an enabled version or an agent-type configuration on the target are skipped.
# The env var name of an agent type is its imAgentType uppercased (NReBPFAgent -> NREBPFAGENT).
def semver: split(".") | map(tonumber? // 0);
def stable: test("^[0-9]+\\.[0-9]+\\.[0-9]+$");
def alive: .enabled and (.eolDate == null or .eolDate > (now | todate));
def env_name: ascii_upcase | gsub("[^A-Z0-9_]"; "_");

[(.targets[$target] // [])[]
 | select((.agentVersion | stable) and alive)]
| group_by(.agentType)
| map(
    sort_by(.agentVersion | semver) | last
    | ([.configurationDefinitions[] | select(.configurationType == "agent-type") | .configurationVersion]
        | sort_by(semver) | last) as $agentTypeVersion
    | select($agentTypeVersion != null)
    | {name: (.agentType | env_name), imAgentType: .agentType, packageVersion: .agentVersion, agentTypeVersion: $agentTypeVersion}
    | if .name | test("^[A-Z_]") | not then error("\(.imAgentType) does not map to a valid env var name") else . end
  )
| group_by(.name)
| map(if length > 1 then error("agent types \(map(.imAgentType) | join(", ")) map to the same env var name \(.[0].name)") else .[0] end)
