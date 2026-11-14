# NixOS VM tests (cgroup v2) for the systemd service shipped in build/package.
# The unit file is the one in the repository; only `KillMode` is varied. A "sleeper" sub-agent
# (a custom agent type running `sleep`) stands in for the real ones, so the tests exercise how
# systemd treats sub-agent processes when Agent Control is killed or stopped.
{ self, inputs, ... }:
let
  vmLib = import ./scenario.nix;
in
{
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      # The test driver runs on the host (QEMU with KVM on Linux, HVF on Darwin); the guest is
      # always Linux of the same architecture.
      inherit (config.ac) guest;
      guestPkgs = inputs.nixpkgs.legacyPackages.${guest.system};
      binaries = self.packages.${guest.system}."${guest.arch}-linux-musl";

      unit = pkgs.runCommand "newrelic-agent-control-unit" { } ''
        mkdir -p $out/lib/systemd/system
        substitute ${../../../build/package/newrelic-agent-control.service} \
          $out/lib/systemd/system/newrelic-agent-control.service \
          --replace-fail /usr/bin/newrelic-agent-control ${binaries}/bin/newrelic-agent-control
      '';

      sleeperAgentType = vmLib.sleeperAgentType {
        inherit pkgs;
        sleepPath = "${guestPkgs.coreutils}/bin/sleep";
      };

      mkTest =
        killMode:
        inputs.nixpkgs.lib.nixos.runTest {
          name = "agent-control-kill-mode-${killMode}";
          hostPkgs = pkgs;
          node.pkgs = guestPkgs;

          nodes.machine =
            { ... }:
            {
              virtualisation.memorySize = 1024;
              virtualisation.cores = 2;

              systemd.packages = [ unit ];
              systemd.services.newrelic-agent-control = {
                wantedBy = [ "multi-user.target" ];
                serviceConfig.KillMode = killMode;
              };

              environment.etc = {
                "newrelic-agent-control/local-data/agent-control/local_config.yaml".source = vmLib.localConfig pkgs;
                "newrelic-agent-control/local-data/sleeper/local_config.yaml".text = "";
                "newrelic-agent-control/dynamic-agent-types/sleeper.yaml".source = sleeperAgentType;
              };
            };

          testScript = vmLib.scenario {
            inherit killMode;
            expectedLayout = "v2";
          };
        };
    in
    {
      checks = lib.genAttrs' vmLib.killModes (killMode: {
        name = "vm-nixos-kill-mode-${killMode}";
        value = mkTest killMode;
      });
    };
}
