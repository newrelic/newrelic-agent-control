# Development shell with the pinned toolchain and the tools CI uses. The build inputs of the
# on-host package (cmake, pkg-config, libiconv on Darwin) come from the package itself.
_: {
  perSystem =
    {
      config,
      pkgs,
      ...
    }:
    {
      devShells.default = config.ac.craneLib.devShell {
        inputsFrom = [ config.packages.default ];

        # Hooks are defined in hooks/ but not installed on entry: the hooks directory is
        # shared by all worktrees, and installing there would break those without this flake.
        packages = [
          config.pre-commit.settings.package
          config.treefmt.build.wrapper
          pkgs.actionlint
          pkgs.cargo-deny
          pkgs.cargo-llvm-cov
          pkgs.cargo-nextest
          pkgs.cargo-shear
          pkgs.git
          pkgs.gnumake
          pkgs.shellcheck
        ];

        RUST_SRC_PATH = "${config.ac.toolchain}/lib/rustlib/src/rust/library";
      };
    };
}
