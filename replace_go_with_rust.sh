#!/bin/sh

BINARY_PATH="./dist/newrelic-supervisor_linux_${ARCH}/newrelic-supervisor"
RUST_VERSION="1.69.0"

# remove go generated files
rm "${BINARY_PATH}"

# compile production version of rust agent

docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/newrelic-supervisor -w /usr/src/newrelic-supervisor rust:${RUST_VERSION} cargo build --release

# moe rust compiled files into goreleaser generated locations
cp ./target/release/main "${BINARY_PATH}"