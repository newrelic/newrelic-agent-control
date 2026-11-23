# Development shell with the pinned toolchain and the tools CI uses. The build inputs of the
# on-host package (libiconv on Darwin) come from the package itself.
_: {
  perSystem =
    {
      config,
      pkgs,
      ...
    }:
    {
      devShells.dev = config.rustLib.craneLib.devShell {
        inputsFrom = [ config.packages.default ];

        # git-hooks.nix defines the hooks and hooks.nix installs them, not this shell: `.git/hooks`
        # is shared by all worktrees, so installing here would change the commits of worktrees
        # that are not in the shell.
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
        ]
        ++ config.pre-commit.settings.enabledPackages;
      };
    };
}
