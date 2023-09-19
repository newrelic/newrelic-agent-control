#!/usr/bin/env bash
set -e

RUST_VERSION="1.71.1"

# remove go generated files
# rm -rf ./target/**

# compile production version of rust agent

if [ "$ARCH" = "arm64" ];then
  BINARY_PATH="./dist/newrelic-super-agent_linux_${ARCH}/newrelic-super-agent"
  ARCH_NAME="aarch64"
fi

if [ "$ARCH" = "amd64" ];then
  BINARY_PATH="./dist/newrelic-super-agent_linux_${ARCH}_v1/newrelic-super-agent"
  ARCH_NAME="x86_64"
fi

#rm "${BINARY_PATH}"

docker build -t "rust-cross-${ARCH_NAME}" -f ./build/rust.Dockerfile --build-arg ARCH_NAME="${ARCH_NAME}" SSH_KEY_RUST_CRATES=${SSH_KEY_RUST_CRATES} .

if [ "x${CI}" = "xtrue" ]; then
    CARGO_HOME=/tmp/.cargo cargo fetch
    docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app -v /tmp/.cargo:/usr/src/app/.cargo rust-cross-"${ARCH_NAME}"
else
    docker run --rm --user "$(id -u)":"$(id -g)" -v "$PWD":/usr/src/app rust-cross-"${ARCH_NAME}"
fi

# move rust compiled files into goreleaser generated locations
cp "./target/${ARCH_NAME}-unknown-linux-gnu/release/main" "${BINARY_PATH}"

# validate
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "apt-get update && apt-get install tree -y && tree ./"
