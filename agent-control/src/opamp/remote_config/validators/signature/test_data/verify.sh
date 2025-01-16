#!/bin/bash
# This script helps to manually verify the signature of a Remote config for testing proposes.
# example:
# ServerToAgent: {
#       remote_config:{
#             config: {
#                   config_map: {
#                         "agentConfig": {
#                               body: "chart_version: 1.10.12\nchart_values:\n  podLabels: \"192.168.5.0\""
#                               content_type: ""
#                         }
#                   }
#             }
#             config_hash: "817982697f614312018935c351fdd71aca190f106fda2d7bc69da86e878ea5e4"
#       }
#       custom_message:{
#             capability: "com.newrelic.security.configSignature"
#             type: "newrelicRemoteConfigSignature"
#             data: {
#                   "3936250589": [{
#                         "checksum":  "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
#                         "checksumAlgorithm":  "SHA256",
#                         "signature":  "nppw2CuZg+YO5MsEoNOsHlgHxF7qAwWPli37NGXAr5isfP1jUTSJcLi0l7k9lNlpbq31GF9DZ0JQBZhoGS0j+sDjvirKSb7yXdqj6JcZ8sxax7KWAnk5QPiwLHFA1kGmszVJ/ccbwtVozG46FvKedcc3X5RME/HGdJupKBe3UzmJawL0xs9jNY+9519CL+CpbkBl/WgCvrIUhTNZv5TUHK23hMD+kz1Brf60pW7MQVtsyClOllsb6WhAsSXdhkpSCJ+96ZGyYywUlvx3/vkBM5a7q4IWqiPM4U0LPZDMQJQCCpxWV3T7cnIR1Ye2yYUqJHs9vfKmTWeBKH2Tb5FgpQ==",
#                         "signingAlgorithm": "RSA_PKCS1_2048_SHA256",
#                         "signatureSpecification": "PKCS #1 v2.2",
#                         "signingDomain": "iast-csec-se.test-poised-pear.cell.us.nr-data.net",
#                         "keyID":  "778b223984d389ad6555bdbbbf118420290c53296b6511e1964309965ec5f710"
#                   }]
#             }
#       }
# }


mkdir -p verify && cd verify

cert_url="iast-csec-se.test-poised-pear.cell.us.nr-data.net:443"

signature_base64="nppw2CuZg+YO5MsEoNOsHlgHxF7qAwWPli37NGXAr5isfP1jUTSJcLi0l7k9lNlpbq31GF9DZ0JQBZhoGS0j+sDjvirKSb7yXdqj6JcZ8sxax7KWAnk5QPiwLHFA1kGmszVJ/ccbwtVozG46FvKedcc3X5RME/HGdJupKBe3UzmJawL0xs9jNY+9519CL+CpbkBl/WgCvrIUhTNZv5TUHK23hMD+kz1Brf60pW7MQVtsyClOllsb6WhAsSXdhkpSCJ+96ZGyYywUlvx3/vkBM5a7q4IWqiPM4U0LPZDMQJQCCpxWV3T7cnIR1Ye2yYUqJHs9vfKmTWeBKH2Tb5FgpQ=="

openssl s_client -connect $cert_url </dev/null 2>/dev/null | openssl x509 -inform pem -text | sed -n '/-----BEGIN CERTIFICATE-----/,/-----END CERTIFICATE-----/p' > public_cert.pem
openssl x509 -inform pem -in public_cert.pem -pubkey -noout > certificate_publickey.pem

echo $signature_base64 > signature.base64
openssl base64 -d -in signature.base64 -out signature.sha256

echo -en "chart_version: 1.10.12\nchart_values:\n  podLabels: \"192.168.5.0\"" > msg

openssl dgst -sha256 -verify certificate_publickey.pem  -signature signature.sha256  msg

