#!/bin/bash
# This script verifies a signature using a public key extracted from a certificate

mkdir -p verify && cd verify

cert_url="iast-csec-se.test-poised-pear.cell.us.nr-data.net:443"

signature_base64="nppw2CuZg+YO5MsEoNOsHlgHxF7qAwWPli37NGXAr5isfP1jUTSJcLi0l7k9lNlpbq31GF9DZ0JQBZhoGS0j+sDjvirKSb7yXdqj6JcZ8sxax7KWAnk5QPiwLHFA1kGmszVJ/ccbwtVozG46FvKedcc3X5RME/HGdJupKBe3UzmJawL0xs9jNY+9519CL+CpbkBl/WgCvrIUhTNZv5TUHK23hMD+kz1Brf60pW7MQVtsyClOllsb6WhAsSXdhkpSCJ+96ZGyYywUlvx3/vkBM5a7q4IWqiPM4U0LPZDMQJQCCpxWV3T7cnIR1Ye2yYUqJHs9vfKmTWeBKH2Tb5FgpQ=="

openssl s_client -connect $cert_url </dev/null 2>/dev/null | openssl x509 -inform pem -text | sed -n '/-----BEGIN CERTIFICATE-----/,/-----END CERTIFICATE-----/p' > public_cert.pem
openssl x509 -inform pem -in public_cert.pem -pubkey -noout > certificate_publickey.pem

echo $signature_base64 > signature.base64
openssl base64 -d -in signature.base64 -out signature.sha256

echo -en "chart_version: 1.10.12\nchart_values:\n  podLabels: \"192.168.5.0\"" > msg

openssl dgst -sha256 -verify certificate_publickey.pem  -signature signature.sha256  msg

