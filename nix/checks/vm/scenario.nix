# Pieces shared by the VM tests: a "sleeper" sub-agent and the scenario run against the systemd
# service. Not a module: imported by nixos.nix and distros.nix.
let
  sleepSeconds = "31337";

  # Adoption of surviving sub-agents is not implemented yet; the related assertions are
  # kept off until it is.
  adoptionImplemented = false;
in
{
  killModes = [
    "mixed"
    "process"
  ];

  # A custom agent type that runs `sleep`, standing in for the real sub-agents.
  sleeperAgentType =
    { pkgs, sleepPath }:
    pkgs.writeText "sleeper-agent-type.yaml" ''
      namespace: external
      name: com.example.sleeper
      version: 0.0.1
      protocol_version: "1.0"
      platform: host
      operating_system: linux
      deployment:
        executables:
          - id: sleeper
            path: ${sleepPath}
            args:
              - "${sleepSeconds}"
            restart_policy:
              backoff_strategy:
                type: fixed
                backoff_delay: 5s
    '';

  localConfig =
    pkgs:
    pkgs.writeText "local_config.yaml" ''
      agents:
        sleeper:
          agent_type: "external/com.example.sleeper:0.0.1"
    '';

  # Python test script (scenario.py) with its constants. `expectedLayout` is the cgroup
  # hierarchy of the host: v2, hybrid or v1.
  scenario =
    { killMode, expectedLayout }:
    ''
      KILL_MODE = "${killMode}"
      EXPECTED_LAYOUT = "${expectedLayout}"
      ADOPTION = ${if adoptionImplemented then "True" else "False"}
      SLEEP_SECONDS = "${sleepSeconds}"
    ''
    + builtins.readFile ./scenario.py;
}
