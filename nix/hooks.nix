# Git hooks installed on `nix develop`: formatting on commit, clippy on push.
{ inputs, ... }:
{
  imports = [ inputs.git-hooks.flakeModule ];

  perSystem =
    { ac, config, ... }:
    {
      pre-commit.settings.hooks = {
        treefmt = {
          enable = true;
          package = config.treefmt.build.wrapper;
        };

        cargo-clippy = {
          enable = true;
          name = "cargo clippy";
          entry = "${ac.toolchain}/bin/cargo clippy --workspace --all-targets -- -D warnings -D clippy::all";
          files = "\\.rs$|Cargo\\.(toml|lock)$";
          pass_filenames = false;
          stages = [ "pre-push" ];
        };
      };
    };
}
