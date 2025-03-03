TERRAFORM_DIR := ./terraform
HELM_DIR := ./helm
AWS_REGION := us-east-2
.DEFAULT_GOAL := all

# Generate a random key to add to the helm deployment annotation
# This way, whenever there is a helm upgrade the pod will be recreated and its image pulled, even if the tag didn't change
DEPLOYMENT_KEY := $(shell openssl rand -base64 32 | tr -dc A-Za-z0-9 | head -c 16)

.PHONY: all
all:
	@echo "No default target"

.PHONY: test/k8s-canaries/terraform-plan
test/k8s-canaries/terraform-plan:
ifndef CANARY_DIR
	@echo "CANARY_DIR missing"
	exit 1
endif
ifndef NEW_RELIC_ACCOUNT_ID
	@echo "NEW_RELIC_ACCOUNT_ID missing"
	exit 1
endif
ifndef NEW_RELIC_API_KEY
	@echo "NEW_RELIC_API_KEY missing"
	exit 1
endif
ifndef SLACK_WEBHOOK_URL
	@echo SLACK_WEBHOOK_URL missing"
	exit 1
endif
	@terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) plan \
	-var="api_key=$(NEW_RELIC_API_KEY)" -var="account_id=$(NEW_RELIC_ACCOUNT_ID)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)"

.PHONY: test/k8s-canaries/terraform-apply
test/k8s-canaries/terraform-apply:
ifndef CANARY_DIR
	@echo "CANARY_DIR missing"
	exit 1
endif
ifndef NEW_RELIC_ACCOUNT_ID
	@echo "NEW_RELIC_ACCOUNT_ID missing"
	exit 1
endif
ifndef NEW_RELIC_API_KEY
	@echo "NEW_RELIC_API_KEY missing"
	exit 1
endif
ifndef SLACK_WEBHOOK_URL
	@echo SLACK_WEBHOOK_URL missing"
	exit 1
endif
	@terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) apply -auto-approve \
	-var="api_key=$(NEW_RELIC_API_KEY)" -var="account_id=$(NEW_RELIC_ACCOUNT_ID)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)"

.PHONY: test/k8s-canaries/update-kubeconfig-from-aws
test/k8s-canaries/update-kubeconfig-from-aws:
ifndef CLUSTER_NAME
	@echo "CLUSTER_NAME variable must be provided for test/k8s-canaries/aws-eks"
	exit 1
endif
	@aws eks update-kubeconfig --region=$(AWS_REGION) --name $(CLUSTER_NAME)

.PHONY: test/k8s-canaries/set-persistent-system-identity
test/k8s-canaries/set-persistent-system-identity:
ifndef NR_SYSTEM_IDENTITY_CLIENT_ID
	@echo "NR_SYSTEM_IDENTITY_CLIENT_ID missing"
	exit 1
endif
ifndef NR_SYSTEM_IDENTITY_PRIVATE_KEY
	@echo "NR_SYSTEM_IDENTITY_PRIVATE_KEY missing"
	exit 1
endif
	@kubectl create namespace newrelic --dry-run=client -o yaml | kubectl apply -f -
	@echo -n $$NR_SYSTEM_IDENTITY_PRIVATE_KEY > /tmp/private_key
	@kubectl get secret sys-identity --namespace newrelic || \
    kubectl create secret generic sys-identity \
    --namespace newrelic \
    --from-literal=CLIENT_ID=$$NR_SYSTEM_IDENTITY_CLIENT_ID \
    --from-file=private_key=/tmp/private_key

.PHONY: test/k8s-canaries/helm-upgrade
test/k8s-canaries/helm-upgrade: test/k8s-canaries/update-kubeconfig-from-aws test/k8s-canaries/set-persistent-system-identity
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY missing"
	exit 1
endif
ifndef CLUSTER_NAME
	@echo "CLUSTER_NAME missing"
	exit 1
endif
ifndef IMAGE_TAG
	@echo "IMAGE_TAG missing"
	exit 1
endif
ifndef FLEET_ID
	@echo "FLEET_ID missing"
	exit 1
endif
ifneq ($(IS_STAGING),true)
ifneq ($(IS_STAGING),false)
	@echo "IS_STAGING variable must be either 'true' or 'false'. Got '$(IS_STAGING)'"
	exit 1
endif
endif
	@kubectl create namespace newrelic --dry-run=client -o yaml | kubectl apply -f -
	@helm repo add newrelic https://helm-charts.newrelic.com
	@helm upgrade --install ac newrelic/agent-control --devel -f $(HELM_DIR)/agent-control.yml \
    --namespace newrelic \
    --set global.licenseKey=$(NR_LICENSE_KEY) \
    --set global.cluster=$(CLUSTER_NAME) \
    --set global.nrStaging=$(IS_STAGING) \
    --set agent-control-deployment.image.tag=$(IMAGE_TAG) \
    --set agent-control-deployment.config.fleet_control.auth.secret.create=false \
    --set agent-control-deployment.config.fleet_control.auth.secret.name="sys-identity" \
    --set agent-control-deployment.config.fleet_control.fleet_id="$(FLEET_ID)" \
    --set agent-control-deployment.podAnnotations.deploymentKey="$(DEPLOYMENT_KEY)"
