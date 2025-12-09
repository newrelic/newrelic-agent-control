#!/bin/bash
set -e

# Exit with no error if signing should be skipped
if [ -n "$SKIP_WINDOWS_SIGN" ]; then
    echo "Skipping Windows executable signing (SKIP_WINDOWS_SIGN is set)"
    exit 0
fi

# Check that required env variables are set
if [ -z "$EXECUTABLE" ] || [ -z "$PFX_CERTIFICATE_BASE64" ] || [ -z "$PFX_PASSPHRASE" ]; then
    echo "EXECUTABLE, PFX_CERTIFICATE_BASE64 and PFX_PASSPHRASE env variables are required"
    exit 1
fi

PFX_CERTIFICATE_DESCRIPTION="New Relic"

# Build the docker image for windows signing
CURRENT_DIR="$( dirname $( readlink -f ${BASH_SOURCE[0]} ) )"
IMAGE_NAME="exec-windows-signer"
docker build -t "$IMAGE_NAME" "$CURRENT_DIR/."

# Sign the binary
EXEC_PARENT_DIR="$(dirname "$EXECUTABLE")"
EXEC_FILE_NAME="$(basename "$EXECUTABLE")"
docker run --rm \
    -v "$EXEC_PARENT_DIR:/workdir" \
    -w /workdir \
    -e PFX_CERTIFICATE_BASE64="$PFX_CERTIFICATE_BASE64" \
    -e PFX_PASSPHRASE="$PFX_PASSPHRASE" \
    -e PFX_CERTIFICATE_DESCRIPTION="$PFX_CERTIFICATE_DESCRIPTION" \
    -e EXECUTABLE="$EXEC_FILE_NAME" \
    "$IMAGE_NAME"
