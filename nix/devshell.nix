# Development shell with the pinned toolchain and the tools CI uses.
_: {
  perSystem =
    {
      ac,
      pkgs,
      config,
      ...
    }:
    {
      devShells.default = pkgs.mkShell {
        # Hooks are defined in hooks.nix but not installed on entry: the hooks directory is
        # shared by all worktrees, and installing there would break those without this flake.
        packages = [
          ac.toolchain
          config.pre-commit.settings.package
          config.treefmt.build.wrapper
          pkgs.cargo-deny
          pkgs.cargo-llvm-cov
          pkgs.cargo-nextest
          pkgs.cargo-shear
          pkgs.cmake
          pkgs.pkg-config
          pkgs.git
          pkgs.gnumake
          pkgs.shellcheck
          pkgs.actionlint
        ]
        ++ ac.commonArgs.buildInputs;

        RUST_SRC_PATH = "${ac.toolchain}/lib/rustlib/src/rust/library";
      };
    };
}
