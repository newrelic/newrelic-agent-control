# Include Ansible dependencies common installation strategy
include test/Ansible.common

TERRAFORM_DIR ?= ./terraform
ONHOST_ANSIBLE_FOLDER ?= ./ansible

.DEFAULT_GOAL := all

.PHONY: all
all:
	@echo "No default target"

.PHONY: test/onhost-canaries/terraform-plan
test/onhost-canaries/terraform-plan:
ifndef ENVIRONMENT
	@echo "ENVIRONMENT missing"
	exit 1
endif
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY missing"
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
	@echo "SLACK_WEBHOOK_URL missing"
	exit 1
endif
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_license_key=${NR_LICENSE_KEY} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_slack_webhook_url=${SLACK_WEBHOOK_URL} \
	terraform -chdir=$(TERRAFORM_DIR) plan -var-file "environments/$(ENVIRONMENT)/inputs.tfvars" 

.PHONY: test/onhost-canaries/terraform-apply
test/onhost-canaries/terraform-apply:
ifndef ENVIRONMENT
	@echo "ENVIRONMENT missing"
	exit 1
endif
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY missing"
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
ifndef NEW_RELIC_API_KEY
	@echo "NEW_RELIC_API_KEY missing"
	exit 1
endif
ifndef SLACK_WEBHOOK_URL
	@echo "SLACK_WEBHOOK_URL missing"
	exit 1
endif
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ONHOST_ANSIBLE_FOLDER) ANSIBLE_ENV_SCRIPT=$(ONHOST_ANSIBLE_FOLDER)/set_ansible_env.sh
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
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
# This env var is required to install agent control with ansible, not to build the infrastructure
ifndef NR_SYSTEM_IDENTITY_PRIVATE_KEY
	@echo "NR_SYSTEM_IDENTITY_PRIVATE_KEY missing"
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
	TF_VAR_license_key=${NR_LICENSE_KEY} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_slack_webhook_url=${SLACK_WEBHOOK_URL} \
    terraform -chdir=$(TERRAFORM_DIR) destroy -auto-approve -var-file "environments/$(ENVIRONMENT)/inputs.tfvars"
