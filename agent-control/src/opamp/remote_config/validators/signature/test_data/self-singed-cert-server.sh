#!/bin/sh

# This script spawn a docker nginx server with a self-signed certificate
# and create a configuration file and sign it with the certificate private key
# for testing proposes.

mkdir -p self-cert-server && cd self-cert-server

# source: https://users.rust-lang.org/t/use-tokio-tungstenite-with-rustls-instead-of-native-tls-for-secure-websockets/90130
# Create unencrypted private key and a CSR (certificate signing request)
openssl req -newkey rsa:2048 -nodes -subj "/C=FI/CN=testname" -keyout server.key -out key.csr

# Create self-signed certificate (`server.crt`) with the private key and CSR
openssl x509 -signkey server.key -in key.csr -req -days 365 -out server.crt

# Create a self-signed root CA
openssl req -x509 -sha256 -nodes -subj "/C=FI/CN=caname" -days 1825 -newkey rsa:2048 -keyout ca.key -out ca.crt

# Create file localhost.ext with the following content:
cat <<'EOF' >> localhost.ext
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
subjectAltName = @testname
[testname]
DNS.1 = localhost
EOF

# Sign the CSR (`server.crt`) with the root CA certificate and private key
# => this overwrites `server.crt` because it gets signed
openssl x509 -req -CA ca.crt -CAkey ca.key -in key.csr -out server.crt -days 365 -CAcreateserial -extfile localhost.ext

# Create nginx configuration file to start a tls server using the generated certificates
cat <<'EOF' >> nginx.conf
events {
    # You can leave this block empty or configure worker connections
    worker_connections 1024;
}

http {
    server {
        listen 443 default_server ssl;
        server_name test;

        ssl_certificate /etc/nginx/certs/server.crt;
        ssl_certificate_key /etc/nginx/certs/server.key;

        location / {
            root /usr/share/nginx/html;
            index index.html;
        }
    }
}
EOF

# Create a dummy configuration file and sign it with the private key and verify it just for
# testing proposes
cat <<'EOF' > config.yaml
chart_version: 1.10.12
chart_values:
  podLabels: "192.168.5.0"
EOF
openssl dgst -sha256 -sign server.key -out signature.sha256 config.yaml
openssl base64 -in signature.sha256 -out signature.base64

openssl x509 -inform pem -in server.crt -pubkey -noout > server.pub
openssl dgst -sha256 -verify server.pub  -signature signature.sha256  config.yaml

# Run nginx server with the generated certificates
docker run --rm -p 443:443 \
  -v ./:/etc/nginx/certs:ro \
  -v ./nginx.conf:/etc/nginx/nginx.conf:ro \
  nginx
