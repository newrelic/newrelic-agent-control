# Nix Flake Modules

This directory contains modular Nix expressions for the New Relic Agent Control project.

## Structure

```
nix/
├── README.md              # This file
├── rust.nix              # Rust toolchain, crane setup, common build args
├── binary-packages.nix   # Cross-platform binary builds
├── distro-packages.nix   # DEB/RPM package generation
└── devshell.nix          # Development environment
```

## Module Descriptions

### rust.nix
**Purpose**: Rust toolchain configuration and shared build infrastructure

**Exports**:
- `rustToolchain` - Rust toolchain with version from Cargo.toml
- `rustVersion` - The Rust version being used
- `craneLib` - Crane library for Rust builds
- `buildInputs` - Runtime dependencies
- `baseBuildInputs` - Build-time dependencies
- `devTools` - Development tools (cargo-watch, cargo-edit, etc.)
- `src` - Filtered source tree
- `agentControlCargoToml` - Parsed Cargo.toml metadata
- `commonArgs` - Shared build arguments
- `cargoArtifacts` - Pre-built dependencies

### binary-packages.nix
**Purpose**: Build binary packages for multiple platforms

**Exports**:
- `mkPackage` - Function to create a binary package for a target
- `packages` - Attribute set of binary packages:
  - `x86_64-linux-musl` - Static Linux binary (x86_64)
  - `aarch64-linux-musl` - Static Linux binary (ARM64)
  - `x86_64-windows-msvc` - Windows binary
  - `default` - Native build for current platform

### distro-packages.nix
**Purpose**: Create distribution-specific packages (DEB/RPM)

**Exports**:
- `mkLinuxPackage` - Function to create DEB/RPM from binary package
- `packages` - Attribute set of distro packages:
  - `deb-x86_64`, `deb-aarch64` - Debian packages
  - `rpm-x86_64`, `rpm-aarch64` - RPM packages
  - `all-linux-binaries` - All Linux binaries combined
  - `all-deb-packages` - All DEB packages combined
  - `all-rpm-packages` - All RPM packages combined
  - `all-linux-packages` - Everything combined

### devshell.nix
**Purpose**: Development environment configuration

**Exports**: A configured `mkShell` derivation with:
- Rust toolchain and development tools
- Build dependencies (zig, cargo-zigbuild)
- Environment variables
- Shell hook with helpful information

## Usage

These modules are imported by the top-level `flake.nix`:

```nix
let
  rust = import ./nix/rust.nix { inherit pkgs crane rust-overlay; };
  binaryPackages = import ./nix/binary-packages.nix { inherit pkgs rust; };
  distroPackages = import ./nix/distro-packages.nix {
    inherit pkgs rust;
    binaryPackages = binaryPackages.packages;
  };
  devshell = import ./nix/devshell.nix { inherit pkgs rust; };
in
{
  devShells.default = devshell;
  packages = binaryPackages.packages // distroPackages.packages;
}
```

## Adding New Packages

To add a new binary target, edit `binary-packages.nix`:

```nix
packages = {
  # ... existing packages ...
  
  your-new-target = mkPackage {
    target = "your-rust-target";
    zigTarget = "your-zig-target";  # optional
    isWindows = false;               # optional
  };
};
```

To add a new distribution format, edit `distro-packages.nix` and add a `mkLinuxPackage` call.

## Benefits of This Structure

1. **Modularity**: Each concern (toolchain, binaries, distros, dev) is isolated
2. **Reusability**: Functions like `mkPackage` are easy to find and modify
3. **Maintainability**: Changes to one area don't affect others
4. **Readability**: File names clearly indicate purpose
5. **Testability**: Can evaluate individual modules independently
6. **Scalability**: Easy to add new targets or package formats

## File Size Comparison

- **Original flake.nix**: 447 lines (monolithic)
- **New structure**: 
  - `flake.nix`: 52 lines (orchestration only)
  - `nix/rust.nix`: 95 lines
  - `nix/binary-packages.nix`: 90 lines
  - `nix/distro-packages.nix`: 180 lines
  - `nix/devshell.nix`: 50 lines
  - **Total**: 467 lines (slightly more due to module overhead, but much better organized)
