#!/usr/bin/env bash

# This script will download the nr-otel-collector config and use it to create the values.yaml sample
COLLECTOR_VERSION="0.5.0"
AGENT_TYPE_VERSION="0.1.0"
BUILD_TMP_FOLDER=build-tmp
URL="https://raw.githubusercontent.com/newrelic/opentelemetry-collector-releases/nr-otel-collector-${COLLECTOR_VERSION}/configs"
DEFAULT_CONFIG_FILENAME="nr-otel-collector-agent-linux.yaml"
GATEWAY_CONFIG_FILENAME="nr-otel-collector-gateway.yaml"
CONFIG_PLACEHOLDER="OTEL_COLLECTOR_CONFIG"
TEMPLATE_DIR="build/example_templates"
OUT_DIR="build/examples"
TEMPLATE_DEFAULT="${TEMPLATE_DIR}/values-nr-otel-collector-agent-linux-${AGENT_TYPE_VERSION}.yaml"
TEMPLATE_GATEWAY="${TEMPLATE_DIR}/values-nr-otel-collector-gateway-${AGENT_TYPE_VERSION}.yaml"
FINAL_DEFAULT="${OUT_DIR}/values-nr-otel-collector-agent-linux-${AGENT_TYPE_VERSION}.yaml"
FINAL_GATEWAY="${OUT_DIR}/values-nr-otel-collector-gateway-${AGENT_TYPE_VERSION}.yaml"
OUTPUT_FILE="${BUILD_TMP_FOLDER}/examples/values-nr-otel-collector-${AGENT_TYPE_VERSION}.yaml"

# Copy templates into their final locations (ignored by git)
cp ${TEMPLATE_DEFAULT} ${FINAL_DEFAULT}
cp ${TEMPLATE_GATEWAY} ${FINAL_GATEWAY}

mkdir -p ${BUILD_TMP_FOLDER}

# Download nr-otel-collector config
curl -s -o "${BUILD_TMP_FOLDER}/${DEFAULT_CONFIG_FILENAME}" "${URL}/${DEFAULT_CONFIG_FILENAME}"
# Download nr-otel-collector config
curl -s -o "${BUILD_TMP_FOLDER}/${GATEWAY_CONFIG_FILENAME}" "${URL}/${GATEWAY_CONFIG_FILENAME}"
# Add spaces to be embedded in the values.yaml
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "sed -i 's/^/  /g' ${BUILD_TMP_FOLDER}/${DEFAULT_CONFIG_FILENAME}"
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "sed -i 's/^/  /g' ${BUILD_TMP_FOLDER}/${GATEWAY_CONFIG_FILENAME}"
# Replace nr-otel-collector sample file placeholder with nr-otel-config
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "sed -i -e '/${CONFIG_PLACEHOLDER}/{r ${BUILD_TMP_FOLDER}/${DEFAULT_CONFIG_FILENAME}' -e 'd}' ${FINAL_DEFAULT}"
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "sed -i -e '/${CONFIG_PLACEHOLDER}/{r ${BUILD_TMP_FOLDER}/${GATEWAY_CONFIG_FILENAME}' -e 'd}' ${FINAL_GATEWAY}"



