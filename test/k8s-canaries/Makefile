TERRAFORM_DIR := ./terraform
HELM_DIR := ./helm
.DEFAULT_GOAL := all

# Generate a random key to add to the helm deployment annotation
# This way, whenever there is a helm upgrade the pod will be recreated and its image pulled, even if the tag didn't change
DEPLOYMENT_KEY := $(shell openssl rand -base64 32 | tr -dc A-Za-z0-9 | head -c 16)

.PHONY: all
all:
	@echo "No default target"

.PHONY: test/k8s-canaries/sync
test/k8s-canaries/sync:
ifndef CANARY_DIR
	@echo "CANARY_DIR variable must be provided to know which canary to sync"
	exit 1
endif
	terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) apply -auto-approve

.PHONY: test/k8s-canaries/helm
test/k8s-canaries/helm:
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY variable must be provided for k8s-canaries/helm"
	exit 1
endif
ifndef CLUSTER_NAME
	@echo "CLUSTER_NAME variable must be provided for k8s-canaries/helm"
	exit 1
endif
ifndef IMAGE_TAG
	@echo "IMAGE_TAG variable must be provided for k8s-canaries/helm"
	exit 1
endif
	@helm repo add newrelic https://helm-charts.newrelic.com
	@helm upgrade --install ac newrelic/agent-control --devel -f $(HELM_DIR)/agent-control.yml \
    -n newrelic --create-namespace \
    --set global.licenseKey=$(NR_LICENSE_KEY) \
    --set global.cluster=$(CLUSTER_NAME) \
    --set agent-control-deployment.image.tag=$(IMAGE_TAG) \
    --set agent-control-deployment.podAnnotations.deploymentKey="${DEPLOYMENT_KEY}"
