# VM tests on non-NixOS distributions with numtide/nix-vm-test: the deb or rpm built from this
# revision is installed with the distribution's package tool and the same scenario as the NixOS
# tests is run, on hosts with different cgroup layouts.
{ inputs, ... }:
let
  vmLib = import ./scenario.nix;

  # `share` is how the package and test files reach the guest: a 9p shared directory, or HTTP
  # served by the test driver for kernels built without 9p (RHEL and its clones).
  distros = {
    ubuntu-20_04 = {
      image = "ubuntu";
      version = "20_04";
      format = "deb";
      share = "9p";
      layout = "hybrid";
    };
    ubuntu-22_04 = {
      image = "ubuntu";
      version = "22_04";
      format = "deb";
      share = "9p";
      layout = "v2";
    };
    rocky-9_6 = {
      image = "rocky";
      version = "9_6";
      format = "rpm";
      share = "http";
      layout = "v2";
    };
    # RHEL 8 compatible, cgroup v1. Its aarch64 image does not boot on Apple silicon (64 KiB pages).
    rocky-8_9 = {
      image = "rocky";
      version = "8_9";
      format = "rpm";
      share = "http";
      layout = "v1";
      systems = [ "x86_64-linux" ];
    };
  };

  installCommand = {
    deb = "dpkg -i /mnt/pkg/*.deb";
    rpm = "rpm -i /mnt/pkg/*.rpm";
  };
in
{
  perSystem =
    {
      config,
      pkgs,
      lib,
      system,
      ...
    }:
    let

      configDir = pkgs.runCommand "newrelic-agent-control-test-config" { } ''
        mkdir -p $out
        cp ${
          vmLib.sleeperAgentType {
            inherit pkgs;
            sleepPath = "/bin/sleep";
          }
        } $out/sleeper-agent-type.yaml
        cp ${vmLib.localConfig pkgs} $out/local_config.yaml
      '';

      package = distro: config.packages."${distro.format}-${config.ac.guest.arch}";

      # Served to guests without 9p support.
      serveDir =
        distro:
        pkgs.runCommand "newrelic-agent-control-test-files" { } ''
          mkdir -p $out/pkg
          cp ${package distro}/*.${distro.format} $out/pkg/pkg.${distro.format}
          cp -r ${configDir} $out/config
        '';

      # Runs on the host, inside the test driver (fetch_over_http.py).
      fetchOverHttp =
        distro:
        ''
          SERVE_DIR = "${serveDir distro}"
          FORMAT = "${distro.format}"
        ''
        + builtins.readFile ./fetch_over_http.py;

      mkTest =
        distro: killMode:
        let
          test = inputs.nix-vm-test.lib.${system}.${distro.image}.${distro.version} {
            memorySize = 2048;
            cpus = 2;
            sharedDirs = lib.optionalAttrs (distro.share == "9p") {
              pkg = {
                source = "${package distro}";
                target = "/mnt/pkg";
              };
              config = {
                source = "${configDir}";
                target = "/mnt/config";
              };
            };
            testScript = ''
              machine = vm
              vm.wait_for_unit("multi-user.target")

              ${lib.optionalString (distro.share == "http") (fetchOverHttp distro)}

              # The package starts the service with an empty `agents` list.
              vm.succeed("${installCommand.${distro.format}}")
              vm.succeed("""
                set -e
                cd /etc/newrelic-agent-control
                mkdir -p dynamic-agent-types local-data/sleeper
                cp /mnt/config/sleeper-agent-type.yaml dynamic-agent-types/sleeper.yaml
                install -m 0600 /mnt/config/local_config.yaml local-data/agent-control/local_config.yaml
                : > local-data/sleeper/local_config.yaml
                mkdir -p /etc/systemd/system/newrelic-agent-control.service.d
                printf '[Service]\nKillMode=${killMode}\n' > /etc/systemd/system/newrelic-agent-control.service.d/kill-mode.conf
                systemctl daemon-reload
                systemctl restart newrelic-agent-control.service
              """)

              ${vmLib.scenario {
                inherit killMode;
                expectedLayout = distro.layout;
              }}
            '';
          };
        in
        # The HTTP server in the test driver is reached over the loopback interface.
        test.sandboxed.overrideAttrs { __darwinAllowLocalNetworking = true; };
    in
    {
      checks = lib.concatMapAttrs (
        name: distro:
        lib.optionalAttrs (lib.elem system (distro.systems or [ system ])) (
          lib.genAttrs' vmLib.killModes (killMode: {
            name = "vm-${name}-kill-mode-${killMode}";
            value = mkTest distro killMode;
          })
        )
      ) distros;
    };
}
