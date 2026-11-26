# Cargo steps of CI that `nix flake check` runs: rustfmt and clippy.
# treefmt and git-hooks contribute their own checks from their modules.
_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      inherit (config) rustLib;
      inherit (rustLib) craneLib;
      workspaceArgs = rustLib.commonArgs // {
        cargoExtraArgs = "--workspace";
        # Tests rely on debug assertions, as in CI.
        CARGO_PROFILE = "dev";
      };
      cargoArtifacts = craneLib.buildDepsOnly workspaceArgs;
      # This test counts reports over real timers, which tick late in the macOS build sandbox while
      # passing in the dev shell.
      darwinExclusions = lib.optionalString pkgs.stdenv.hostPlatform.isDarwin " and not test(=agent_control::uptime_report::tests::test_uptime_report)";
    in
    {
      checks = {
        cargo-clippy = craneLib.cargoClippy (
          workspaceArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings -D clippy::all";
          }
        );

        # Tests that need no root, network or cluster: every workspace crate but agent-control in
        # full, and the agent-control lib. Its integration suites run in their own CI jobs.
        cargo-nextest = craneLib.cargoNextest (
          workspaceArgs
          // {
            inherit cargoArtifacts;
            # The HTTP clients load the system CA roots when built, and the sandbox has none.
            SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
            cargoNextestExtraArgs = lib.concatStringsSep " " ([
              "--all-targets"
              "--no-fail-fast"
              "--retries 2"
              "-E"
              "'not (package(newrelic_agent_control) and not kind(lib))${darwinExclusions}'"
            ]);
            preCheck = ''
              echo testing-machine-id > "$TMPDIR/machine-id"
              export TEST_MACHINE_ID_PATH="$TMPDIR/machine-id"
            '';
          }
        );

        cargo-fmt = craneLib.cargoFmt {
          inherit (rustLib) src;
          inherit (rustLib.commonArgs) pname version;
        };
      };
    };
}
