#!/usr/bin/env bash

set -e

if [ "$ARCH" = "arm64" ];then
  ARCH_DIRNAME="./dist/${BIN}_linux_${ARCH}_v8.0"
fi

if [ "$ARCH" = "amd64" ];then
  ARCH_DIRNAME="./dist/${BIN}_linux_${ARCH}_v1"
fi

# move rust compiled files into goreleaser generated locations
cp "./bin/${BIN}-${ARCH}" "${ARCH_DIRNAME}/${BIN}"

# validate files are in the correct location (if tree is installed)
if command -v tree >/dev/null 2>&1; then
  tree ./dist/
fi
