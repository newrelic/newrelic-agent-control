#!/bin/sh

BINARY_PATH="./dist/newrelic-supervisor_linux_${ARCH}/newrelic-supervisor"
RUST_VERSION="1.69.0"

# remove go generated files
rm "${BINARY_PATH}"

# compile production version of rust agent

if [ "$ARCH" == "arm64" ];then

  docker build -t rust-cross-aarch64 -f ./rust-aarch64.Dockerfile .
  docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app rust-cross-aarch64

fi

if [ "$ARCH" == "amd64" ];then

  docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app -w /usr/src/app rust:${RUST_VERSION} cargo build --release

fi

# move rust compiled files into goreleaser generated locations
cp ./target/release/main "${BINARY_PATH}"

# download assets
