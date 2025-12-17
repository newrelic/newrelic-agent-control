#!/bin/bash
set -e

# Obtain the certificate from base64
echo "$PFX_CERTIFICATE_BASE64" | base64 -d > ./certificate.pfx

# Sign the binary with the osslsigncode tool
osslsigncode sign \
    -pkcs12 ./certificate.pfx \
    -pass "$PFX_PASSPHRASE" \
    -n "$PFX_CERTIFICATE_DESCRIPTION" \
    -t http://timestamp.digicert.com \
    -in "$EXECUTABLE" \
    -out "$EXECUTABLE.signed"

# Clean up the certificate file
rm -f ./certificate.pfx

# Replace the unsigned binary by the signed one
mv "$EXECUTABLE.signed" "$EXECUTABLE"
