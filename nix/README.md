# Nix flake

The flake gives every contributor, and CI, the same pinned toolchain and the same commands. What runs on a laptop is what runs on a runner.

## Commands

| Command | What it does |
| --- | --- |
| `nix develop` | Shell with the pinned Rust toolchain (version read from `rust-version` in `Cargo.toml`), `cargo-deny`, `cargo-nextest`, `cargo-llvm-cov`, `cargo-shear`, `shellcheck`, `actionlint`, `treefmt`, `prek`, `git` and `gnumake`. |
| `nix develop .#hooks` | The same shell, and on entry git-hooks writes `.pre-commit-config.yaml` and the hooks of the clone's shared `.git/hooks`. |
| `nix fmt` | Formats Rust, Nix, TOML, YAML, Terraform, shell and the Tiltfiles (treefmt). |
| `nix flake check` | Formatting, git hooks, `cargo fmt` and `cargo clippy --workspace --all-targets -D warnings`, as in CI. |
| `nix build` | Native on-host binaries (`newrelic-agent-control` and `newrelic-agent-control-cli`) for the current system. |
| `nix build .#x86_64-linux-musl` | Static x86_64 Linux binaries, the production target (Linux hosts). |
| `nix build .#aarch64-linux-musl` | Static aarch64 Linux binaries (Linux hosts). |

From a Mac, build for Linux through a Linux builder by selecting the system explicitly, e.g. `nix build .#packages.aarch64-linux.aarch64-linux-musl`.

## Layout

`flake.nix` imports `nix/`, whose `default.nix` imports one module per kind of flake output. A directory holds the modules of one kind and its `default.nix` lists them; a kind that is a single file is its own module. To add a package, a check, a hook or a shell, add a file to the directory of its kind and list it there. Every file is a [flake-parts](https://flake.parts) module.

| Path | Purpose |
| --- | --- |
| `rust-lib/pkgs-overlay.nix` | The `pkgs` of every module: nixpkgs with rust-overlay. |
| `rust-lib/options.nix` | The typed `rustLib` options, readable from any module as `config.rustLib.<name>`. |
| `rust-lib/toolchain.nix` | Defines them: Rust toolchain (version from `Cargo.toml`), crane, source filter and shared build arguments. |
| `packages/` | `binaries.nix`: native and musl builds of the on-host binaries. |
| `devshells/` | `dev.nix`: the development shell; `hooks.nix`: the same shell, with the git hooks installed on entry. |
| `formatting.nix` | treefmt: `rustfmt` from the pinned toolchain, `nixfmt`, `taplo`, `prettier` for YAML, `shfmt`, `terraform` and `tilt` for the Tiltfiles. |
| `git-hooks.nix` | git hooks (prek): treefmt, codespell, shellcheck, actionlint, yamllint, hadolint, statix, deadnix, the `check-*` hooks, `cargo check` and clippy. |
| `checks/` | CI-equivalent checks run by `nix flake check`: `cargo.nix` has `cargo fmt` and clippy. |

The default dev shell does not install the hooks, because `.git/hooks` is shared by all worktrees. Use `nix develop .#hooks` in the clone where you want them: the hooks are installed once, but `.pre-commit-config.yaml` is written in the worktree the shell is entered from, so enter it in every worktree you commit from. `prek uninstall` removes the hooks.

The Windows and Kubernetes binaries are not built by this flake.
