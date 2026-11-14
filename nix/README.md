# Nix flake

The flake gives every contributor, and CI, the same pinned toolchain and the same commands. What runs on a laptop is what runs on a runner.

## Commands

| Command | What it does |
| --- | --- |
| `nix develop` | Shell with the pinned Rust toolchain (version read from `Cargo.toml`), `cargo-deny`, `cargo-nextest`, `cargo-llvm-cov`, `cargo-shear`, `shellcheck`, `actionlint`. |
| `nix fmt` | Formats Rust, Nix and Python files (treefmt). |
| `nix flake check` | Formatting, git hooks, `cargo fmt` and `cargo clippy --workspace --all-targets -D warnings`, as in CI. |
| `nix build` | On-host binaries for the current system. |
| `nix build .#x86_64-linux-musl` | Static x86_64 Linux binaries, the production target (Linux hosts). |
| `nix build .#aarch64-linux-musl` | Static aarch64 Linux binaries (Linux hosts). |
| `nix build .#deb-x86_64`, `.#rpm-x86_64`, `.#deb-aarch64`, `.#rpm-aarch64` | Packages built with nfpm from the musl binaries, using the `nfpms` section of `.goreleaser.yml` for layout, scripts and relations. |
| `nix build .#checks.<system>.vm-nixos-kill-mode-mixed` and `...-process` | NixOS VM tests of the packaged systemd unit with `KillMode=mixed` and `KillMode=process`: sub-agent fate after `kill -9`, `systemctl stop` and `systemctl kill --kill-who=all`. They run on a Mac (aarch64 Linux guest under HVF) and on Linux with KVM. |
| `nix build .#checks.<system>.vm-<distro>-kill-mode-<mode>` | The same scenario on Ubuntu 20.04 (hybrid cgroups), Ubuntu 22.04 and Rocky 9.6 (cgroup v2), installing the deb or rpm built from this revision, plus Rocky 8.9 (cgroup v1) on x86_64 Linux. Built on [nix-vm-test](https://github.com/numtide/nix-vm-test); runs from a Mac through numtide/nix-vm-test#203. |

From a Mac, build for Linux through a Linux builder by selecting the system explicitly, e.g. `nix build .#packages.aarch64-linux.aarch64-linux-musl`.

## Layout

Each directory is one kind of flake output, and its `default.nix` lists the files in it: `flake.nix` imports `nix/`, which imports the directories, which import their files. To add a package, a check, a hook or a shell, add a file to the directory of its kind and list it in that `default.nix`. Every file is a [flake-parts](https://flake.parts) module.

| Path | Purpose |
| --- | --- |
| `pkgs.nix` | The `pkgs` of every module: nixpkgs with rust-overlay. |
| `ac/options.nix` | The typed `ac` options, readable from any module as `config.ac.<name>`. |
| `ac/toolchain.nix` | Defines them: Rust toolchain (version from `Cargo.toml`), crane, source filter and shared build arguments. |
| `ac/guest.nix` | `ac.guest`: the Linux system (and its architecture) packages and VM tests run on; aarch64 Linux on Darwin hosts. |
| `packages/` | `binaries.nix`: native and musl builds of the on-host binaries. `linux-packages.nix`: deb and rpm packages derived from `.goreleaser.yml` (`nfpm.jq` maps its `nfpms` section to an nfpm config). |
| `devshells/` | `dev.nix`: the development shell. |
| `formatting/` | treefmt: `rustfmt` from the pinned toolchain, `nixfmt` and `ruff format`. |
| `hooks/` | git hooks: treefmt on commit, clippy on push. |
| `checks/` | CI-equivalent checks run by `nix flake check`: `cargo.nix` has rustfmt and clippy. |
| `checks/vm/` | VM tests of the service unit and sub-agent process handling. `nixos.nix` runs them on NixOS and `distros.nix` on Ubuntu and Rocky guests installing the packages; `scenario.nix`, `scenario.py` and `fetch_over_http.py` are the sleeper sub-agent, the test scenario and the file delivery to guests, not modules. |

The Windows and Kubernetes binaries are not built by this flake.
