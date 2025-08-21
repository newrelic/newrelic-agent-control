#!/usr/bin/env bash
set -e

# This script assumes that the following tools are installed:
# - The Rust toolchain with the targets for cross-compilation:
#   - `aarch64-unknown-linux-musl`.
#   - `x86_64-unknown-linux-musl`.
# - `zig` for using it as linker.
# - `cargo-zigbuild` for building.

if [ "$ARCH" = "arm64" ];then
  ARCH_NAME="aarch64"
  TARGET_TUPLE="aarch64-unknown-linux-musl"
fi

if [ "$ARCH" = "amd64" ];then
  ARCH_NAME="x86_64"
  TARGET_TUPLE="x86_64-unknown-linux-musl"
fi

if [ "$BUILD_MODE" = "debug" ];then
  BUILD_MODE="dev"
  BUILD_OUT_DIR="debug"
fi
# compile release version if not specified
: "${BUILD_MODE:=release}"
: "${BUILD_OUT_DIR:=release}"

echo "arch: ${ARCH}, arch_name: ${ARCH_NAME}"

# Binary metadata
GIT_COMMIT=$( git rev-parse HEAD )
export GIT_COMMIT
export AGENT_CONTROL_VERSION=${AGENT_CONTROL_VERSION}

export RUSTFLAGS="-C target-feature=+crt-static"

if [[ "$OSTYPE" == "darwin"* ]]; then
  echo "macOS detected, increasing ulimit to 4096 for concurrent Zig linking"
  ulimit -n 4096
fi
cargo zigbuild --target "${TARGET_TUPLE}" --profile "${BUILD_MODE}" --package "${PKG}" --bin "${BIN}"

mkdir -p "bin"

# Handle the cases of the two newrelic-agent-control binaries:
# - newrelic-agent-control-k8s
# - newrelic-agent-control-onhost
# When any of these two is found, rename them to newrelic-agent-control
TRIMMED_BIN="${BIN}"
if [ "$BIN" = "newrelic-agent-control-onhost" ] || [ "$BIN" = "newrelic-agent-control-k8s" ]; then
  TRIMMED_BIN="newrelic-agent-control"
fi

# Copy the generated binaries to the bin directory
cp "./target/${TARGET_TUPLE}/${BUILD_OUT_DIR}/${BIN}" "./bin/${TRIMMED_BIN}-${ARCH}"
