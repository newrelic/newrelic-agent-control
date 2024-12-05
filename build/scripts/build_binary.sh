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
export SUPER_AGENT_VERSION=${SUPER_AGENT_VERSION}

export RUSTFLAGS="-C target-feature=+crt-static"
export CROSS_CONFIG=${CROSS_CONFIG:-"./Cross.toml"}
cross build --target "${ARCH_NAME}-unknown-linux-musl" --profile "${BUILD_MODE}" --features "${BUILD_FEATURE}" --package "${PKG}" --bin "${BIN}"

mkdir -p "bin"

# Copy the generated binaries to the bin directory
cp "./target/${ARCH_NAME}-unknown-linux-musl/${BUILD_OUT_DIR}/${BIN}" "./bin/${BIN}-${ARCH}"
