# Get the absolute path to the current directory
CURR_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

K8S_TF_DIR := $(CURR_DIR)terraform
K8S_HELM_DIR := $(CURR_DIR)helm

AWS_REGION := us-east-2
.DEFAULT_GOAL := all

# Generate a random key to add to the helm deployment annotation
# This way, whenever there is a helm upgrade the pod will be recreated and its image pulled, even if the tag didn't change
DEPLOYMENT_KEY := $(shell openssl rand -base64 32 | tr -dc A-Za-z0-9 | head -c 16)

# If the environment is production, overwrite the variables with the production values
# This is useful on GHA, where both the staging and production secrets are available
ifeq ($(CANARY_DIR), production)
	IS_STAGING = false

	NR_LICENSE_KEY = $(NR_PROD_LICENSE_KEY)
	NEW_RELIC_ACCOUNT_ID = $(NEW_RELIC_PROD_ACCOUNT_ID)
	NEW_RELIC_API_KEY = $(NEW_RELIC_PROD_API_KEY)
	NR_SYSTEM_IDENTITY_CLIENT_ID = $(NR_PROD_SYSTEM_IDENTITY_CLIENT_ID)
	NR_SYSTEM_IDENTITY_PRIVATE_KEY = $(NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY)

# Re-export for k8s secret
	export NR_SYSTEM_IDENTITY_CLIENT_ID
	export NR_SYSTEM_IDENTITY_PRIVATE_KEY
else
	IS_STAGING = true
endif

.PHONY: all
all:
	@echo "No default target"

define check_env_var
	@! test -n "$$$(1)" && echo "$1 missing" && exit 1
endef

define check_tf_env_vars
	$(call check_env_var,CANARY_DIR)
	if [ "$$CANARY_DIR" = "staging" ]; then \
		$(call check_env_var,NEW_RELIC_ACCOUNT_ID); \
		$(call check_env_var,NEW_RELIC_API_KEY); \
	elif [ "$$CANARY_DIR" = "production" ]; then \
		$(call check_env_var,NEW_RELIC_PROD_ACCOUNT_ID); \
		$(call check_env_var,NEW_RELIC_PROD_API_KEY); \
	else \
		echo "CANARY_DIR must be either 'staging' or 'production'. Got '$(CANARY_DIR)'"; \
		exit 1; \
	fi
	$(call check_env_var,SLACK_WEBHOOK_URL)
endef

.PHONY: test/k8s-canaries/terraform-plan
test/k8s-canaries/terraform-plan:
	@$(call check_tf_env_vars)
	@terraform -chdir=$(K8S_TF_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(K8S_TF_DIR)/$(CANARY_DIR) plan \
	-var="api_key=$(NEW_RELIC_API_KEY)" -var="account_id=$(NEW_RELIC_ACCOUNT_ID)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)"

.PHONY: test/k8s-canaries/terraform-apply
test/k8s-canaries/terraform-apply:
	@$(call check_tf_env_vars)
	@terraform -chdir=$(K8S_TF_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(K8S_TF_DIR)/$(CANARY_DIR) apply -auto-approve \
	-var="api_key=$(NEW_RELIC_API_KEY)" -var="account_id=$(NEW_RELIC_ACCOUNT_ID)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)"

.PHONY: test/k8s-canaries/update-kubeconfig-from-aws
test/k8s-canaries/update-kubeconfig-from-aws:
	@$(call check_env_var,CLUSTER_NAME)
	@aws eks update-kubeconfig --region=$(AWS_REGION) --name $(CLUSTER_NAME)

.PHONY: test/k8s-canaries/set-persistent-system-identity
test/k8s-canaries/set-persistent-system-identity:
ifeq ($(CANARY_DIR), staging)
	@$(call check_env_var,NR_SYSTEM_IDENTITY_CLIENT_ID)
	@$(call check_env_var,NR_SYSTEM_IDENTITY_PRIVATE_KEY)
else ifeq ($(CANARY_DIR), production)
	@$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_CLIENT_ID)
	@$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY)
else
	@echo "CANARY_DIR must be either 'staging' or 'production'. Got '$(CANARY_DIR)'"
	@exit 1
endif
	@kubectl create namespace newrelic --dry-run=client -o yaml | kubectl apply -f -
	@echo $$NR_SYSTEM_IDENTITY_PRIVATE_KEY > /tmp/private_key
	@kubectl get secret sys-identity --namespace newrelic || \
    kubectl create secret generic sys-identity \
    --namespace newrelic \
    --from-literal=CLIENT_ID=$$NR_SYSTEM_IDENTITY_CLIENT_ID \
    --from-file=private_key=/tmp/private_key

.PHONY: test/k8s-canaries/helm-upgrade
test/k8s-canaries/helm-upgrade: test/k8s-canaries/update-kubeconfig-from-aws test/k8s-canaries/set-persistent-system-identity
ifeq ($(CANARY_DIR), staging)
	@$(call check_env_var,NR_LICENSE_KEY)
else ifeq ($(CANARY_DIR), production)
	@$(call check_env_var,NR_PROD_LICENSE_KEY)
else
	@echo "CANARY_DIR must be either 'staging' or 'production'. Got '$(CANARY_DIR)'"
	@exit 1
endif
	@$(call check_env_var,CLUSTER_NAME)
	@$(call check_env_var,IMAGE_TAG)
	@$(call check_env_var,FLEET_ID)
	@kubectl create namespace newrelic --dry-run=client -o yaml | kubectl apply -f -
	@helm repo add newrelic https://helm-charts.newrelic.com
	@helm upgrade --install ac newrelic/agent-control --devel -f $(K8S_HELM_DIR)/agent-control.yml \
    --namespace newrelic \
    --set global.licenseKey=$(NR_LICENSE_KEY) \
    --set global.cluster=$(CLUSTER_NAME) \
    --set global.nrStaging=$(IS_STAGING) \
    --set agent-control-deployment.image.tag=$(IMAGE_TAG) \
    --set agent-control-deployment.config.fleet_control.auth.secret.create=false \
    --set agent-control-deployment.config.fleet_control.auth.secret.name="sys-identity" \
    --set agent-control-deployment.config.fleet_control.fleet_id="$(FLEET_ID)" \
    --set agent-control-deployment.podAnnotations.deploymentKey="$(DEPLOYMENT_KEY)"
