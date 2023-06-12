#!/usr/bin/env bash

STAGING="${STAGING:-false}"
ARCH="${ARCH:-amd64}"
GOCACHE=/tmp/.gocache

# download assets
docker run --rm --user "$(id -u)":"$(id -g)" -e GOCACHE="$GOCACHE" -e ARCH="$ARCH" -e STAGING="$STAGING" -v "$PWD":/usr/src/app -w /usr/src/app golang:1.20 make build/embedded

