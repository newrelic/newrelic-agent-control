#!/usr/bin/env bash

ARCH="${ARCH:-amd64}"
GOCACHE=/tmp/.gocache
GOVERSION="${GOVERSION:-1.25}"
# Extract only the number part if GOVERSION starts with 'go', common in the CLI output
GOVERSION="${GOVERSION#go}"

# download assets
docker run --rm --user "$(id -u)":"$(id -g)" -e GOCACHE="$GOCACHE" -e ARCH="$ARCH" -e ARTIFACTS_VERSIONS="${ARTIFACTS_VERSIONS}" -v "$PWD":/usr/src/app -w /usr/src/app golang:"$GOVERSION" make build/embedded

# validate
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "apt-get update && apt-get install tree -y && tree ./build/embedded"
