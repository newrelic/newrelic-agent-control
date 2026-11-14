# Git hooks: formatting on commit, clippy on push. They are defined here and exposed as a flake
# check, but not installed by the dev shell (see devshells/dev.nix).
{ inputs, ... }:
{
  imports = [ inputs.git-hooks.flakeModule ];

  perSystem =
    { config, ... }:
    {
      pre-commit.settings.hooks = {
        treefmt = {
          enable = true;
          package = config.treefmt.build.wrapper;
        };

        cargo-clippy = {
          enable = true;
          name = "cargo clippy";
          entry = "${config.ac.toolchain}/bin/cargo clippy --workspace --all-targets -- -D warnings -D clippy::all";
          files = "\\.rs$|Cargo\\.(toml|lock)$";
          pass_filenames = false;
          stages = [ "pre-push" ];
        };
      };
    };
}
