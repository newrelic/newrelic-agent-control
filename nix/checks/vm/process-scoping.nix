# VM tests of the process scoping proof of concept (process-scoping-poc): each way of starting a
# sub-agent inside a cgroup is run under a supervisor that is killed with SIGKILL and stopped,
# on hosts with every cgroup layout. One VM per distribution runs all backends.
{ inputs, ... }:
let
  distros = import ./distro-list.nix;
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
      poc = config.packages."scoping-poc-${config.ac.guest.arch}-linux-musl";

      # Served to guests without 9p support.
      serveDir = pkgs.runCommand "process-scoping-poc-files" { } ''
        mkdir -p $out/poc
        cp ${poc}/bin/process-scoping-poc $out/poc/
      '';

      fetchOverHttp = ''
        import functools
        import http.server
        import threading

        server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0),
            functools.partial(http.server.SimpleHTTPRequestHandler, directory="${serveDir}"),
        )
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"http://10.0.2.2:{server.server_address[1]}"
        vm.succeed("mkdir -p /mnt/poc")
        vm.succeed(f"curl -fsS -o /mnt/poc/process-scoping-poc {base}/poc/process-scoping-poc")
      '';

      mkTest =
        distro:
        let
          test = inputs.nix-vm-test.lib.${system}.${distro.image}.${distro.version} {
            memorySize = 2048;
            cpus = 2;
            sharedDirs = lib.optionalAttrs (distro.share == "9p") {
              poc = {
                source = "${poc}/bin";
                target = "/mnt/poc";
              };
            };
            testScript = ''
              machine = vm
              EXPECTED_LAYOUT = "${distro.layout}"
              vm.wait_for_unit("multi-user.target")

              ${lib.optionalString (distro.share == "http") fetchOverHttp}
              ${builtins.readFile ./process_scoping.py}
            '';
          };
        in
        test.sandboxed.overrideAttrs { __darwinAllowLocalNetworking = true; };
    in
    {
      checks = lib.concatMapAttrs (
        name: distro:
        lib.optionalAttrs (lib.elem system (distro.systems or [ system ])) {
          "vm-process-scoping-${name}" = mkTest distro;
        }
      ) distros;
    };
}
