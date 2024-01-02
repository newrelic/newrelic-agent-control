#!/usr/bin/env bash
set -e

if [ "$ARCH" = "arm64" ];then
  BINARY_PATH="./dist/${BIN}_linux_${ARCH}/${BIN}"
fi

if [ "$ARCH" = "amd64" ];then
  BINARY_PATH="./dist/${BIN}_linux_${ARCH}_v1/${BIN}"
fi

# move rust compiled files into goreleaser generated locations
cp "./bin/${BIN}-${ARCH}" "${BINARY_PATH}"

# validate
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "apt-get update && apt-get install tree -y && tree ./"
