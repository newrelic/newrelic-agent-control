#!/usr/bin/env bash

STAGING="${STAGING:-false}"
ARCH="${ARCH:-amd64}"
GOCACHE=/tmp/.gocache

# download assets
docker run --rm --user "$(id -u)":"$(id -g)" -e GOCACHE="$GOCACHE" -e ARCH="$ARCH" -e STAGING="$STAGING" -e ARTIFACTS_VERSIONS="${ARTIFACTS_VERSIONS}" -v "$PWD":/usr/src/app -w /usr/src/app golang:1.20 make build/embedded

# validate
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "apt-get update && apt-get install tree -y && tree ./build/embedded"