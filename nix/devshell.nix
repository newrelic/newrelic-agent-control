# Development shell configuration
{ pkgs, rust }:

let
  inherit (rust)
    rustToolchain
    rustVersion
    buildInputs
    baseBuildInputs
    devTools
    ;
in
pkgs.mkShell {
  inherit buildInputs;
  nativeBuildInputs =
    baseBuildInputs
    ++ devTools
    ++ [
      pkgs.zig
      pkgs.cargo-zigbuild
    ];

  # Environment variables
  RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

  # Workaround for macOS file descriptor limit issue with cargo-zigbuild
  shellHook = ''
    echo "🦀 New Relic Agent Control Development Environment"
    echo "📦 Rust version: ${rustVersion}"
    echo "🔧 Rust toolchain: ${rustToolchain}"

    # Increase file descriptor limit on macOS
    if [[ "$OSTYPE" == "darwin"* ]]; then
      ulimit -n 4096 2>/dev/null || true
      echo "⚠️  File descriptor limit set to 4096 (required for cargo-zigbuild on macOS)"
    fi

    echo ""
    echo "Available commands:"
    echo "  cargo build                       - Build the project"
    echo "  cargo zigbuild --target <T>       - Cross-compile (T=x86_64-unknown-linux-musl, etc.)"
    echo "  cargo test --workspace            - Run tests"
    echo "  make coverage                     - Generate coverage report"
    echo "  make third-party-notices          - Check third-party licenses"
    echo ""
    echo "Build with Nix:"
    echo "  nix build .#x86_64-linux-musl      - Build x86_64 Linux static binary"
    echo "  nix build .#aarch64-linux-musl     - Build aarch64 Linux static binary"
    echo "  nix build .#x86_64-windows-msvc    - Build x86_64 Windows binary"
    echo "  nix build .#deb-x86_64             - Build x86_64 DEB package"
    echo "  nix build .#deb-aarch64            - Build aarch64 DEB package"
    echo "  nix build .#rpm-x86_64             - Build x86_64 RPM package"
    echo "  nix build .#rpm-aarch64            - Build aarch64 RPM package"
    echo "  nix build .#all-linux-packages     - Build all Linux packages (binaries + deb + rpm)"
    echo ""
  '';
}
