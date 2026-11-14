# Nix flake

The flake gives every contributor, and CI, the same pinned toolchain and the same commands. What runs on a laptop is what runs on a runner.

## Commands

| Command | What it does |
| --- | --- |
| `nix develop` | Shell with the pinned Rust toolchain (version read from `Cargo.toml`), `cargo-deny`, `cargo-nextest`, `cargo-llvm-cov`, `cargo-shear`, `shellcheck`, `actionlint`. |
| `nix fmt` | Formats Rust and Nix files (treefmt). |
| `nix flake check` | Formatting, git hooks, `cargo fmt` and `cargo clippy --workspace --all-targets -D warnings`, as in CI. |
| `nix build` | On-host binaries for the current system. |
| `nix build .#x86_64-linux-musl` | Static x86_64 Linux binaries, the production target. |
| `nix build .#aarch64-linux-musl` | Static aarch64 Linux binaries. |
| `nix build .#x86_64-windows-gnu` | Windows binaries (`.exe`) for the MinGW target `x86_64-pc-windows-gnu`. |
| `nix build .#deb-x86_64`, `.#rpm-x86_64`, `.#deb-aarch64`, `.#rpm-aarch64` | Packages built with nfpm from the musl binaries, using the `nfpms` section of `.goreleaser.yml` for layout, scripts and relations. |

The musl and Windows builds are cross-compiled with the toolchains of nixpkgs, so they build on Linux and macOS hosts alike, without a Linux builder. The first build on macOS compiles the cross compilers, as they are not in the public binary cache; a shared cache avoids that for everyone else. The MSVC Windows target needs the Microsoft SDK, so it is still built outside Nix.

## Layout

Each directory is one kind of flake output, and its `default.nix` lists the files in it: `flake.nix` imports `nix/`, which imports the directories, which import their files. To add a package, a check, a hook or a shell, add a file to the directory of its kind and list it in that `default.nix`. Every file is a [flake-parts](https://flake.parts) module.

| Path | Purpose |
| --- | --- |
| `pkgs.nix` | The `pkgs` of every module: nixpkgs with rust-overlay. |
| `ac/options.nix` | The typed `ac` options, readable from any module as `config.ac.<name>`. |
| `ac/toolchain.nix` | Defines them: Rust toolchain (version from `Cargo.toml`), crane, source filter and shared build arguments. |
| `ac/guest.nix` | `ac.guest`: the Linux system (and its architecture) packages and VM tests run on; aarch64 Linux on Darwin hosts. |
| `packages/` | `binaries.nix`: native build of the on-host binaries and cross builds (musl, Windows). `linux-packages.nix`: deb and rpm packages derived from `.goreleaser.yml` (`nfpm.jq` maps its `nfpms` section to an nfpm config). |
| `devshells/` | `dev.nix`: the development shell. |
| `formatting/` | treefmt: `rustfmt` from the pinned toolchain and `nixfmt`. |
| `hooks/` | git hooks: treefmt on commit, clippy on push. |
| `checks/` | CI-equivalent checks run by `nix flake check`: `cargo.nix` has rustfmt and clippy. |

The Kubernetes binaries are not built by this flake.
