# The `ac` options: what the Rust toolchain and build modules offer to the rest of the flake.
# Declared here, defined in the other files of this directory, read as `config.ac.<name>` from
# any perSystem module.
{ lib, ... }:
let
  inherit (lib) mkOption types;
  # Values that are not plain data (functions, attribute sets of functions).
  opaque = types.lazyAttrsOf types.raw;
in
{
  perSystem = _: {
    options.ac = {
      rustVersion = mkOption {
        type = types.str;
        readOnly = true;
        description = "Rust version of the workspace, from `rust-version` in Cargo.toml.";
      };

      toolchain = mkOption {
        type = types.package;
        readOnly = true;
        description = "Rust toolchain at `rustVersion`, with rust-src, rust-analyzer and llvm-tools.";
      };

      mkToolchain = mkOption {
        type = types.functionTo types.package;
        readOnly = true;
        description = "Like `toolchain`, for the list of target triples it is given.";
      };

      craneLib = mkOption {
        type = opaque;
        readOnly = true;
        description = "crane library using `toolchain`.";
      };

      mkCraneLib = mkOption {
        type = types.functionTo opaque;
        readOnly = true;
        description = "crane library using the toolchain for the list of target triples it is given.";
      };

      src = mkOption {
        type = types.raw;
        readOnly = true;
        description = "Workspace sources a build needs: Cargo sources and the agent type registry.";
      };

      commonArgs = mkOption {
        type = opaque;
        readOnly = true;
        description = "Arguments shared by all crane derivations.";
      };
    };
  };
}
