# Git hooks: formatting, lint and cargo checks on commit. They are defined here and exposed as a
# flake check; the `hooks` dev shell installs them (see devshells/hooks.nix).
{ inputs, ... }:
{
  imports = [ inputs.git-hooks.flakeModule ];

  perSystem =
    { config, pkgs, ... }:
    let
      rustToolchain = config.rustLib.toolchain;
    in
    {
      pre-commit.settings = {
        package = pkgs.prek;
        hooks = {
          treefmt = {
            enable = true;
            package = config.treefmt.build.wrapper;
          };

          codespell = {
            enable = true;
            name = "codespell";
            entry = pkgs.lib.getExe pkgs.codespell;
            language = "system";
            pass_filenames = false;
            always_run = true;
          };

          shellcheck.enable = true;
          actionlint.enable = true;

          yamllint = {
            enable = false;
            settings.configPath = ".yamllint.yml";
          };

          # The default `files` only matches a file named `Dockerfile`.
          hadolint = {
            enable = false;
            files = "(^|/)Dockerfile";
          };

          deadnix.enable = true;
          statix.enable = true;
          check-merge-conflicts.enable = true;
          check-added-large-files.enable = true;
          # The opamp and k8s identity tests embed dummy keys on purpose.
          detect-private-keys = {
            enable = true;
            excludes = [
              "^agent-control/src/cli/k8s/system_identity.rs$"
              "^agent-control/tests/k8s/tools/agent_control.rs$"
              "^agent-control/tests/on_host/opamp_auth.rs$"
            ];
          };

          cargo-check = {
            enable = true;
            package = rustToolchain;
          };

          clippy = {
            enable = true;
            packageOverrides = {
              cargo = rustToolchain;
              clippy = rustToolchain;
            };
            settings = {
              denyWarnings = true;
              extraArgs = "--workspace --tests";
            };
          };
        };

        # git-hooks.nix keeps its whole config block under a `settings` option, hence the repeat.
        settings.rust.check.cargoDeps = pkgs.rustPlatform.importCargoLock {
          lockFile = ../Cargo.lock;
          outputHashes = {
            "nr-auth-0.7.0" = "sha256-STV902d0gKk7sFVkqaH1bCTv2S3Yzd+tfprq0F1z1w0=";
            "opamp-client-0.3.0" = "sha256-TyX/GIgsPWbeWxuvRTIKT+Q6z/i/9pHhQPMNewuM2dg=";
            "proto-0.1.0" = "sha256-TyX/GIgsPWbeWxuvRTIKT+Q6z/i/9pHhQPMNewuM2dg=";
          };
        };
      };
    };
}
