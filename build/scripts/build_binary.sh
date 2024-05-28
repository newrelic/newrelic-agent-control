#!/usr/bin/env bash
set -e

# compile production version of rust agent

if [ "$ARCH" = "arm64" ];then
  ARCH_NAME="aarch64"
fi

if [ "$ARCH" = "amd64" ];then
  ARCH_NAME="x86_64"
fi

: "${BUILD_MODE:=release}" # Default to release if not specified

if [ -z "${BIN}" ]; then
    BIN="newrelic-super-agent"
    echo "BIN not provided; defaulting to 'newrelic-super-agent'."
fi

if [ -z "${PKG}" ]; then
    PKG="newrelic_super_agent"
    echo "PKG not provided; defaulting to 'newrelic_super_agent'."
fi

echo "arch: ${ARCH}, arch_name: ${ARCH_NAME}"

docker build --platform linux/amd64 -t "rust-cross-${ARCH_NAME}-${BIN}" \
    -f ./build/rust.Dockerfile \
    --build-arg ARCH_NAME="${ARCH_NAME}" \
    --build-arg BUILD_MODE="${BUILD_MODE}" \
    --build-arg BUILD_FEATURE="${BUILD_FEATURE}" \
    --build-arg BUILD_PKG="${PKG}" \
    --build-arg BUILD_BIN="${BIN}" \
    .

# Binary metadata
GIT_COMMIT=$( git rev-parse HEAD )
SUPER_AGENT_VERSION=${SUPER_AGENT_VERSION:-development}
BUILD_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

CARGO_HOME=/tmp/.cargo cargo fetch
docker run --platform linux/amd64 --rm \
  --user "$(id -u)":"$(id -g)" \
  -e GIT_COMMIT=${GIT_COMMIT} \
  -e SUPER_AGENT_VERSION=${SUPER_AGENT_VERSION} \
  -e BUILD_DATE=${BUILD_DATE} \
  -v "${PWD}":/usr/src/app \
  -v /tmp/.cargo:/usr/src/app/.cargo \
  "rust-cross-${ARCH_NAME}-${BIN}"

mkdir -p "bin"

cp "./target-${BIN}/${ARCH_NAME}-unknown-linux-musl/${BUILD_MODE}/${BIN}" "./bin/${BIN}-${ARCH}"
