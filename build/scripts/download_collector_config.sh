#!/usr/bin/env bash

# This script will download the nr-otel-collector config and use it to create the values.yaml sample
BUILD_TMP_FOLDER=build-tmp
DEFAULT_CONFIG_FILENAME="config.yaml"
URL="https://raw.githubusercontent.com/newrelic/nrdot-collector-releases/refs/tags/${NR_OTEL_COLLECTOR_VERSION}/distributions/nrdot-collector-host/${DEFAULT_CONFIG_FILENAME}"
CONFIG_PLACEHOLDER="OTEL_COLLECTOR_CONFIG"
TEMPLATE_DIR="build/example_templates"
OUT_DIR="build/examples"
TEMPLATE_DEFAULT="${TEMPLATE_DIR}/values-nr-otel-collector-agent-linux.yaml"
FINAL_DEFAULT="${OUT_DIR}/values-nr-otel-collector-agent-linux.yaml"

# Copy templates into their final locations (ignored by git)
cp ${TEMPLATE_DEFAULT} ${FINAL_DEFAULT}

mkdir -p ${BUILD_TMP_FOLDER}

# Download nr-otel-collector config
curl -s -o "${BUILD_TMP_FOLDER}/${DEFAULT_CONFIG_FILENAME}" "${URL}/${DEFAULT_CONFIG_FILENAME}"

# Add spaces to be embedded in the values.yaml
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "sed -i 's/^/  /g' ${BUILD_TMP_FOLDER}/${DEFAULT_CONFIG_FILENAME}"
# Replace nr-otel-collector sample file placeholder with nr-otel-config
docker run --rm -v "$PWD":/usr/src/app -w /usr/src/app ubuntu /bin/bash -c "sed -i -e '/${CONFIG_PLACEHOLDER}/{r ${BUILD_TMP_FOLDER}/${DEFAULT_CONFIG_FILENAME}' -e 'd}' ${FINAL_DEFAULT}"



