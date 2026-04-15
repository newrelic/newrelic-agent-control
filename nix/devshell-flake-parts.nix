# Development shell as a flake-parts module
{ ... }:
{
  perSystem =
    {
      config,
      pkgs,
      rust,
      ...
    }:
    let
      inherit (rust)
        rustToolchain
        rustVersion
        buildInputs
        baseBuildInputs
        devTools
        ;
    in
    {
      devShells.default = pkgs.mkShell {
        inherit buildInputs;
        nativeBuildInputs =
          baseBuildInputs
          ++ devTools
          ++ [
            pkgs.zig
            pkgs.cargo-zigbuild
          ];

        RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

        shellHook = ''
          echo "🦀 New Relic Agent Control Development Environment"
          echo "📦 Rust version: ${rustVersion}"
          echo "🔧 Rust toolchain: ${rustToolchain}"

          if [[ "$OSTYPE" == "darwin"* ]]; then
            ulimit -n 4096 2>/dev/null || true
            echo "⚠️  File descriptor limit set to 4096 (required for cargo-zigbuild on macOS)"
          fi

          echo ""
          echo "Available commands:"
          echo "  cargo build                       - Build the project"
          echo "  cargo zigbuild --target <T>       - Cross-compile"
          echo "  cargo test --workspace            - Run tests"
          echo "  make coverage                     - Generate coverage report"
          echo ""
          echo "Build with Nix:"
          echo "  nix build .#x86_64-linux-musl      - Build x86_64 Linux static binary"
          echo "  nix build .#aarch64-linux-musl     - Build aarch64 Linux static binary"
          echo "  nix build .#x86_64-windows-msvc    - Build x86_64 Windows binary"
          echo "  nix build .#deb-x86_64             - Build x86_64 DEB package"
          echo "  nix build .#all-linux-packages     - Build all Linux packages"
          echo ""
        '';
      };
    };
}
