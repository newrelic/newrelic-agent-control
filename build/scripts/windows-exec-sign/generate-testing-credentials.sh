#!/bin/bash

CURRENT_DIR="$( dirname $( readlink -f ${BASH_SOURCE[0]} ) )"
LOCAL_DIR="$CURRENT_DIR/../../../local/testing-pfx-cert"
IMAGE_NAME="testing-credentials"

rm -rf $LOCAL_DIR && mkdir $LOCAL_DIR

docker build -t $IMAGE_NAME "$CURRENT_DIR/."

docker run --rm -v $LOCAL_DIR:/workdir -w /workdir $IMAGE_NAME bash -c '
# Generate a private key
openssl genrsa -out private.key 2048

# Generate a self-signed certificate (valid for 365 days)
openssl req -new -x509 -key private.key -out certificate.crt -days 365 \
  -subj "/C=US/ST=TestST/L=TestL/O=TestO Org/OU=TestOrg Unit/CN=test.org.site"

PFX_PASSPHRASE="TestPassword123"
PFX_FILE="certificate.pfx"

# Convert to PFX format
openssl pkcs12 -export -out $PFX_FILE \
  -inkey private.key \
  -in certificate.crt \
  -passout pass:$PFX_PASSPHRASE

# Encode as base64
base64 $PFX_FILE > certificate_pfx_base64
'

echo "Testing pfx certificate generated:"
echo "PFX_CERTIFICATE_BASE64: $LOCAL_DIR/certificate_pfx_base64"
echo "PFX_PASSPHRASE: TestPassword123"
