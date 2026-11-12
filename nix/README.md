# Nix flake

The flake gives every contributor, and CI, the same pinned toolchain and the same commands. What runs on a laptop is what runs on a runner.

## Commands

| Command | What it does |
| --- | --- |
| `nix develop` | Shell with the pinned Rust toolchain (version read from `Cargo.toml`), `cargo-deny`, `cargo-nextest`, `cargo-llvm-cov`, `cargo-shear`, `shellcheck`, `actionlint`. Installs the git hooks. |
| `nix fmt` | Formats Rust and Nix files (treefmt). |
| `nix flake check` | Formatting, git hooks, `cargo fmt` and `cargo clippy --workspace --all-targets -D warnings`, as in CI. |
| `nix build` | On-host binaries for the current system. |
| `nix build .#x86_64-linux-musl` | Static x86_64 Linux binaries, the production target (Linux hosts). |
| `nix build .#aarch64-linux-musl` | Static aarch64 Linux binaries (Linux hosts). |

From a Mac, build for Linux through a Linux builder by selecting the system explicitly, e.g. `nix build .#packages.aarch64-linux.aarch64-linux-musl`.

## Layout

| File | Purpose |
| --- | --- |
| `toolchain.nix` | Rust toolchain, crane, source filter and build arguments shared by the other modules (the `ac` module argument). |
| `packages.nix` | Native and musl builds of the on-host binaries. |
| `devshell.nix` | Development shell. |
| `formatting.nix` | treefmt: `rustfmt` from the pinned toolchain and `nixfmt`. |
| `hooks.nix` | git hooks: treefmt on commit, clippy on push. |
| `checks.nix` | CI-equivalent checks run by `nix flake check`. |

The Windows and Kubernetes binaries are not built by this flake.
