#!/usr/bin/env bash
set -e

RUST_VERSION="1.71.1"

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


if [ -z "${BUILD_FEATURE}" ]; then
    BUILD_FEATURE="onhost"
    echo "BUILD_FEATURE not provided; defaulting to 'onhost'."
fi

echo "arch: ${ARCH}, arch_name: ${ARCH_NAME}"

docker build --platform linux/amd64 -t "rust-cross-${ARCH_NAME}-${BIN}" -f ./build/rust.Dockerfile --build-arg ARCH_NAME="${ARCH_NAME}" --build-arg BUILD_MODE="${BUILD_MODE}" --build-arg BUILD_FEATURE="${BUILD_FEATURE}" --build-arg BUILD_BIN="${BIN}" .

CARGO_HOME=/tmp/.cargo cargo fetch
docker run --platform linux/amd64 --rm --user "$(id -u)":"$(id -g)" -v "${PWD}":/usr/src/app -v /tmp/.cargo:/usr/src/app/.cargo "rust-cross-${ARCH_NAME}-${BIN}"

mkdir -p "bin"

cp "./target-${BIN}/${ARCH_NAME}-unknown-linux-musl/${BUILD_MODE}/${BIN}" "./bin/${BIN}-${ARCH}"
