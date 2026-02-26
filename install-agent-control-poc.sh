#!/bin/bash
# Script para instalar Agent Control desde el repositorio PoC de GitHub Actions

set -e

echo "📥 Installing newrelic-cli..."
curl -Ls https://download.newrelic.com/install/newrelic-cli/scripts/install.sh | bash


echo "🚀 executing recipe..."
sudo \
  NEW_RELIC_CLI_SKIP_CORE=1 \
  NEW_RELIC_API_KEY="LICENSE" \
  NEW_RELIC_ACCOUNT_ID="ACCOUNT ID" \
  NEW_RELIC_AUTH_CLIENT_ID="ID" \
  NEW_RELIC_AGENT_VERSION="0.100.23193573416" \
  NEW_RELIC_REGION=US \
  NR_CLI_FLEET_ID="FLEET_ID" \
  NEW_RELIC_AGENT_CONTROL=true \
  NEW_RELIC_ORGANIZATION="ORG" \
  NEW_RELIC_AUTH_CLIENT_SECRET="SECRET" \
  NEW_RELIC_DOWNLOAD_URL="http://nr-downloads-ohai-testing.s3-website-us-east-1.amazonaws.com/poc_ebpf/linux/apt/pool/main/n/newrelic-agent-control/" \
  /usr/local/bin/newrelic install \
  -c debian.yml \
  -y \
  --debug \
  -n agent-control

echo "✅ Installation Complete!"
echo ""
echo "📊 verifying..."
curl -s http://localhost:51200/status '.'
echo ""
echo "📝 Logs: journalctl -u newrelic-agent-control -f"
