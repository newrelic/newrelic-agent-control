#!/usr/bin/env bash
set -e

# Install cargo cross
which cross || cargo install cross

if [ "$ARCH" = "arm64" ];then
  ARCH_NAME="aarch64"
fi

if [ "$ARCH" = "amd64" ];then
  ARCH_NAME="x86_64"
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
export CROSS_CONFIG=${CROSS_CONFIG:-"./Cross.toml"}
cross build --target "${ARCH_NAME}-unknown-linux-musl" --profile "${BUILD_MODE}" --features "${BUILD_FEATURE}" --package "${PKG}" --bin "${BIN}"

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
cp "./target/${ARCH_NAME}-unknown-linux-musl/${BUILD_OUT_DIR}/${BIN}" "./bin/${TRIMMED_BIN}-${ARCH}"
