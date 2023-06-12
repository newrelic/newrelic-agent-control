#!/usr/bin/env bash
set -e

RUST_VERSION="1.69.0"

# remove go generated files
rm -rf ./target/**

# compile production version of rust agent

if [ "$ARCH" = "arm64" ];then
  BINARY_PATH="./dist/nr-meta-agent_linux_${ARCH}/nr-meta-agent"
  rm "${BINARY_PATH}"
  docker build -t rust-cross-aarch64 -f ./build/rust-aarch64.Dockerfile .
  docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app rust-cross-aarch64
  # move rust compiled files into goreleaser generated locations
  cp ./target/aarch64-unknown-linux-gnu/release/main "${BINARY_PATH}"

fi

if [ "$ARCH" = "amd64" ];then
  BINARY_PATH="./dist/nr-meta-agent_linux_${ARCH}_v1/nr-meta-agent"
  rm "${BINARY_PATH}"
  docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app -w /usr/src/app rust:${RUST_VERSION} cargo build --release
  # move rust compiled files into goreleaser generated locations
  cp ./target/release/main "${BINARY_PATH}"
fi


# validate
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "apt-get update && apt-get install tree -y && tree ./"
