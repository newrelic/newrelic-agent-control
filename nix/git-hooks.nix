# Git hooks: formatting on commit, clippy on push. They are defined here and exposed as a flake
# check, but not installed by the dev shell (see devshells/dev.nix).
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
          settings.rust.check.cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ../Cargo.lock; };
        };
      };
    };
}
