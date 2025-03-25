# Include Ansible dependencies common installation strategy
include test/Ansible.common

TERRAFORM_DIR ?= ./terraform
ONHOST_ANSIBLE_FOLDER ?= ./ansible

.DEFAULT_GOAL := all

ifeq ($(ENVIRONMENT), production)
	NR_LICENSE_KEY = $(NR_PROD_LICENSE_KEY)
	NEW_RELIC_ACCOUNT_ID = $(NEW_RELIC_PROD_ACCOUNT_ID)
	NEW_RELIC_API_KEY = $(NEW_RELIC_PROD_API_KEY)
	NR_SYSTEM_IDENTITY_CLIENT_ID = $(NR_PROD_SYSTEM_IDENTITY_CLIENT_ID)
	NR_SYSTEM_IDENTITY_PRIVATE_KEY = $(NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY)
	export NR_SYSTEM_IDENTITY_PRIVATE_KEY
endif

.PHONY: all
all:
	@echo "No default target"

define check_env_var
	if [ -z "$$$(1)" ]; then \
		echo "$1 missing"; \
		exit 1; \
	fi
endef

define check_env_vars
	$(call check_env_var,ENVIRONMENT)
	$(call check_env_var,SLACK_WEBHOOK_URL)
	if [ "$$ENVIRONMENT" = "staging" ]; then \
		$(call check_env_var,NR_LICENSE_KEY); \
		$(call check_env_var,NEW_RELIC_ACCOUNT_ID); \
		$(call check_env_var,NEW_RELIC_API_KEY); \
		$(call check_env_var,NR_SYSTEM_IDENTITY_CLIENT_ID); \
		$(call check_env_var,NR_SYSTEM_IDENTITY_PRIVATE_KEY); \
	fi
	if [ "$$ENVIRONMENT" = "production" ]; then \
		$(call check_env_var,NR_PROD_LICENSE_KEY); \
		$(call check_env_var,NEW_RELIC_PROD_ACCOUNT_ID); \
		$(call check_env_var,NEW_RELIC_PROD_API_KEY); \
		$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_CLIENT_ID); \
		$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY); \
	fi
endef

.PHONY: test/onhost-canaries/terraform-plan
test/onhost-canaries/terraform-plan:
	@$(call check_env_vars)
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
	TF_VAR_license_key=${NR_LICENSE_KEY} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_slack_webhook_url=${SLACK_WEBHOOK_URL} \
	terraform -chdir=$(TERRAFORM_DIR) plan -var-file "environments/$(ENVIRONMENT)/inputs.tfvars" 

.PHONY: test/onhost-canaries/terraform-apply
test/onhost-canaries/terraform-apply:
	@$(call check_env_vars)
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ONHOST_ANSIBLE_FOLDER) ANSIBLE_ENV_SCRIPT=$(ONHOST_ANSIBLE_FOLDER)/set_ansible_env.sh
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
	TF_VAR_license_key=${NR_LICENSE_KEY} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_slack_webhook_url=${SLACK_WEBHOOK_URL} \
	terraform -chdir=$(TERRAFORM_DIR) apply -auto-approve -var-file "environments/$(ENVIRONMENT)/inputs.tfvars"

.PHONY: test/onhost-canaries/terraform-destroy
test/onhost-canaries/terraform-destroy:
ifndef ENVIRONMENT
	@echo "ENVIRONMENT missing"
	exit 1
endif
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY missing"
	exit 1
endif
ifndef NR_SYSTEM_IDENTITY_CLIENT_ID
	@echo "NR_SYSTEM_IDENTITY_CLIENT_ID missing"
	exit 1
endif
# This env var is required to install agent control with ansible, not to build the infrastructure
ifndef NR_SYSTEM_IDENTITY_PRIVATE_KEY
	@echo "NR_SYSTEM_IDENTITY_PRIVATE_KEY missing"
	exit 1
endif
ifndef NEW_RELIC_ACCOUNT_ID
	@echo "NEW_RELIC_ACCOUNT_ID missing"
	exit 1
endif
ifndef NEW_RELIC_ACCOUNT_ID
	@echo "NEW_RELIC_ACCOUNT_ID missing"
	exit 1
endif
ifndef SLACK_WEBHOOK_URL
	@echo "SLACK_WEBHOOK_URL missing"
	exit 1
endif
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
	TF_VAR_license_key=${NR_LICENSE_KEY} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_slack_webhook_url=${SLACK_WEBHOOK_URL} \
    terraform -chdir=$(TERRAFORM_DIR) destroy -auto-approve -var-file "environments/$(ENVIRONMENT)/inputs.tfvars"
