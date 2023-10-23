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

docker build -t "rust-cross-${ARCH_NAME}" -f ./build/rust.Dockerfile --build-arg ARCH_NAME="${ARCH_NAME}" .

CARGO_HOME=/tmp/.cargo cargo fetch
docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app -v /tmp/.cargo:/usr/src/app/.cargo rust-cross-"${ARCH_NAME}"

mkdir -p "bin"

cp "./target/${ARCH_NAME}-unknown-linux-gnu/release/newrelic-super-agent" "./bin/newrelic-super-agent-${ARCH}"
