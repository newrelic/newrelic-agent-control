#!/usr/bin/env bash
set -e

if [ "$ARCH" = "arm64" ];then
  BINARY_PATH="./dist/newrelic-super-agent_linux_${ARCH}/newrelic-super-agent"
fi

if [ "$ARCH" = "amd64" ];then
  BINARY_PATH="./dist/newrelic-super-agent_linux_${ARCH}_v1/newrelic-super-agent"
fi

# move rust compiled files into goreleaser generated locations
cp "./bin/newrelic-super-agent-${ARCH}" "${BINARY_PATH}"

# validate
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "apt-get update && apt-get install tree -y && tree ./"

