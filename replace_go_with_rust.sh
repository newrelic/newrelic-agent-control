#!/bin/sh

BINARY_PATH="./dist/newrelic-supervisor_linux_${ARCH}/newrelic-supervisor"
RUST_VERSION="1.69.0"

# remove go generated files
rm "${BINARY_PATH}"
rm -rf ./target/**

# compile production version of rust agent

if [ "$ARCH" = "arm64" ];then

  docker build -t rust-cross-aarch64 -f ./rust-aarch64.Dockerfile .
  docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app rust-cross-aarch64
  # move rust compiled files into goreleaser generated locations
  cp ./target/aarch64-unknown-linux-gnu/release/main "${BINARY_PATH}"

fi

if [ "$ARCH" = "amd64" ];then

  docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app -w /usr/src/app rust:${RUST_VERSION} cargo build --release
  # move rust compiled files into goreleaser generated locations
  cp ./target/release/main "${BINARY_PATH}"
fi

# move rust compiled files into goreleaser generated locations
cp ./target/release/main "${BINARY_PATH}"

# download assets
docker run --rm --user "$(id -u)":"$(id -g)" -e DOWNLOAD_ARCH="$ARCH" -v "$PWD":/usr/src/app -w /usr/src/app/build/embedded golang:1.20 make run

# validate
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "apt-get update && apt-get install tree -y && tree ./"
