#!/bin/bash
set -e

## This script deploys in different namespaces the helm chart of the agent control
## You need to have the following files in ~/.fleet-control:
## - client_id: the client id of the system identity
## - e2e_key: the private key of the system identity
## - license_key: your new relic license key


echo "Starting deployment of all namespaces..."

helm upgrade --install flux -n load-test newrelic/agent-control-cd --set flux2.watchAllNamespaces=true

cat << EOF > local/load-test.yaml
systemIdentity:
  create: false                  #<-- you need to pre-create it in your cluster
  secretName: "sys-identity"

config:
  acRemoteUpdate: false
  cdRemoteUpdate: false
  fleet_control:
    enabled: true
    # Fleet with name 'load-test'. The sub-agent reports data to the 'Agent Control Canaries' account.
    fleet_id: "NjQyNTg2NXxOR0VQfEZMRUVUfDAxOThjODA5LTlmMWEtNzM4Zi1iMDBjLTVmNDAwZWQ4YTJjZA"
  override:
    fleet_control:
        signature_validation:
          enabled: false
  log:
    level: warn
  agents:
    infra:
      agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
    infra-2:
      agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
agentsConfig:
  infra:
    chart_values:
      newrelic-infrastructure:
        enabled: false
      nri-metadata-injection:
        enabled: false
      kube-state-metrics:
        enabled: false
      nri-kube-events:
        enabled: false
    chart_version: "*"
  infra-2:
    chart_values:
      newrelic-infrastructure:
        enabled: false
      nri-metadata-injection:
        enabled: false
      kube-state-metrics:
        enabled: false
      nri-kube-events:
        enabled: false
    chart_version: "*"
EOF
echo "[$(date)] saved config file"


for i in {1..150}; do
    NAMESPACE="load-test-release-$i"
    RELEASE_NAME="load-test-release-$i"
    CLUSTER_NAME="load-test-release-$i"


    echo "[$(date)] Creating namespace: $NAMESPACE"
    kubectl create namespace $NAMESPACE || true

    echo "[$(date)] Creating identity: $NAMESPACE" -n $NAMESPACE
    kubectl create secret generic sys-identity --namespace $NAMESPACE --from-literal=CLIENT_ID="$(cat ~/.agent-control/client_id)" --from-literal=private_key="$(cat ~/.agent-control/e2e_key)" || true
    echo "[$(date)] Installing helm chart in $NAMESPACE"

    helm upgrade $RELEASE_NAME newrelic/agent-control-deployment --install \
     --namespace $NAMESPACE --values local/load-test.yaml --set cluster=$CLUSTER_NAME --set subAgentsNamespace=$NAMESPACE --set licenseKey="$(cat ~/.agent-control/license_key)"

    echo "[$(date)] Completed $NAMESPACE ($i/150)"
done

echo "All 150 namespaces deployed successfully!"