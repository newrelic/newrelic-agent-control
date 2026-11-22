# Development shell for the clones where the git hooks should be installed: the `dev` shell plus
# git-hooks' installation script, which writes `.pre-commit-config.yaml` and the hooks of the
# shared `.git/hooks` on entry. Those hooks are shared by all worktrees, so installing them is
# opt-in rather than part of `dev`.
_: {
  perSystem =
    { config, ... }:
    {
      devShells.hooks = config.rustLib.craneLib.devShell {
        inputsFrom = [ config.devShells.dev ];
        # The hook entries are absolute store paths, so build them with the shell.
        packages = config.pre-commit.settings.enabledPackages;
        shellHook = config.pre-commit.shellHook;
      };
    };
}
