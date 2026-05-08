# Get the absolute path to the current directory
# It's important to use the `:=` assignment operator to force immediate expansion of `CURR_DIR`
# Otherwise, the `include` directive will modify the `MAKEFILE_LIST` variable
CURR_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

ONHOST_TF_DIR := $(CURR_DIR)terraform
ONHOST_ANSIBLE_FOLDER := $(CURR_DIR)ansible

PACKAGE_VERSION ?=

# Include Ansible dependencies common installation strategy
include $(CURR_DIR)../Ansible.common

# If the environment is production, overwrite the variables with the production values
# This is useful on GHA, where both the staging and production secrets are available
ifeq ($(ENVIRONMENT), production)
	NR_LICENSE_KEY = $(NR_PROD_LICENSE_KEY)
	NEW_RELIC_ACCOUNT_ID = $(NEW_RELIC_PROD_ACCOUNT_ID)
	NEW_RELIC_API_KEY = $(NEW_RELIC_PROD_API_KEY)
	NR_SYSTEM_IDENTITY_CLIENT_ID = $(NR_PROD_SYSTEM_IDENTITY_CLIENT_ID)
	NR_SYSTEM_IDENTITY_PRIVATE_KEY = $(NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY)

# The private key is used in the ansible file, it need to be exported
# for the subshell to use the correct value
	export NR_SYSTEM_IDENTITY_PRIVATE_KEY
endif

.DEFAULT_GOAL := all

.PHONY: all
all:
	@echo "No default target"

define check_env_var
	if [ -z "$$$(1)" ]; then \
		echo "$1 missing"; \
		exit 1; \
	fi
endef

define check_env_vars_base
	$(call check_env_var,ENVIRONMENT)
	if [ "$$ENVIRONMENT" = "staging" ]; then \
    	$(call check_env_var,NEW_RELIC_ACCOUNT_ID); \
    	$(call check_env_var,NEW_RELIC_API_KEY); \
    elif [ "$$ENVIRONMENT" = "production" ]; then \
    	$(call check_env_var,NEW_RELIC_PROD_ACCOUNT_ID); \
    	$(call check_env_var,NEW_RELIC_PROD_API_KEY); \
    else \
    	echo "ENVIRONMENT must be either 'staging' or 'production'"; \
    	exit 1; \
    fi
endef

define check_env_vars_infra
	$(call check_env_vars_base)
	$(call check_env_var,AC_CANARY_WINDOWS_PASSWORD)
	$(call check_env_var,SLACK_WEBHOOK_URL)
endef

define check_env_vars_install
	$(call check_env_vars_base)
    $(call check_env_var,FLEET_ID)
	$(call check_env_var,WINDOWS_FLEET_ID)
	if [ "$$ENVIRONMENT" = "staging" ]; then \
    	$(call check_env_var,NR_LICENSE_KEY); \
    	$(call check_env_var,NR_SYSTEM_IDENTITY_CLIENT_ID); \
    	$(call check_env_var,NR_SYSTEM_IDENTITY_PRIVATE_KEY); \
    elif [ "$$ENVIRONMENT" = "production" ]; then \
    	$(call check_env_var,NR_PROD_LICENSE_KEY); \
    	$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_CLIENT_ID); \
    	$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY); \
    else \
    	echo "ENVIRONMENT must be either 'staging' or 'production'"; \
    	exit 1; \
    fi
endef

.PHONY: test/onhost-canaries/infra-plan
test/onhost-canaries/infra-plan:
	@$(call check_env_vars_infra)
	terraform -chdir=$(ONHOST_TF_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_windows_password="${AC_CANARY_WINDOWS_PASSWORD}" \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_slack_webhook_url="${SLACK_WEBHOOK_URL}" \
	terraform -chdir=$(ONHOST_TF_DIR) plan -var-file "environments/$(ENVIRONMENT)/inputs.tfvars"

.PHONY: test/onhost-canaries/infra-apply
test/onhost-canaries/infra-apply:
	@$(call check_env_vars_infra)
	terraform -chdir=$(ONHOST_TF_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_windows_password="${AC_CANARY_WINDOWS_PASSWORD}" \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_slack_webhook_url="${SLACK_WEBHOOK_URL}" \
	terraform -chdir=$(ONHOST_TF_DIR) apply -auto-approve -var-file "environments/$(ENVIRONMENT)/inputs.tfvars"

.PHONY: test/onhost-canaries/infra-destroy
test/onhost-canaries/infra-destroy:
	@$(call check_env_vars_infra)
	terraform -chdir=$(ONHOST_TF_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_windows_password="${AC_CANARY_WINDOWS_PASSWORD}" \
	TF_VAR_account_id=${NEW_RELIC_ACCOUNT_ID} \
	TF_VAR_api_key=${NEW_RELIC_API_KEY} \
	TF_VAR_slack_webhook_url="${SLACK_WEBHOOK_URL}" \
	terraform -chdir=$(ONHOST_TF_DIR) destroy -auto-approve -var-file "environments/$(ENVIRONMENT)/inputs.tfvars"

.PHONY: test/onhost-canaries/ansible-install
test/onhost-canaries/ansible-install:
	@$(call check_env_vars_install)
	@echo "Generating inventory from Terraform state..."
	@terraform -chdir=$(ONHOST_TF_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf
	@mkdir -p $(ONHOST_TF_DIR)/environments/$(ENVIRONMENT)
	@terraform -chdir=$(ONHOST_TF_DIR) output -raw ansible_inventory_content > \
		$(ONHOST_TF_DIR)/environments/$(ENVIRONMENT)/inventory.$(ENVIRONMENT)
	@echo "✓ Inventory file generated from Terraform state"
	@INVENTORY_FILE=$(ONHOST_TF_DIR)/environments/$(ENVIRONMENT)/inventory.$(ENVIRONMENT) && \
	echo "Validating Ansible inventory contains the 2 canaries..." && \
	HOST_COUNT=$$(ansible-inventory -i $$INVENTORY_FILE --list 2>/dev/null | jq '[.. | .hosts? // empty] | flatten | length') && \
	if [ "$$HOST_COUNT" -ne 2 ]; then \
		echo "ERROR: Inventory file has no hosts defined"; \
		echo "Inventory content:"; \
		cat $$INVENTORY_FILE; \
		exit 1; \
	fi && \
	echo "✓ Inventory validation passed ($$HOST_COUNT hosts found)"
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ONHOST_ANSIBLE_FOLDER) ANSIBLE_ENV_SCRIPT=$(ONHOST_ANSIBLE_FOLDER)/set_ansible_env.sh
	@echo "Running Ansible playbook for environment: $(ENVIRONMENT)"
	@INVENTORY_FILE=$(ONHOST_TF_DIR)/environments/$(ENVIRONMENT)/inventory.$(ENVIRONMENT) && \
	echo "Using inventory: $$INVENTORY_FILE" && \
	ANSIBLE_CONFIG=$(ONHOST_ANSIBLE_FOLDER)/ansible.cfg \
	ANSIBLE_HOST_KEY_CHECKING=False ansible-playbook \
		-i $$INVENTORY_FILE \
		--private-key ~/.ssh/caos-dev-arm.cer \
		-e @$(ONHOST_ANSIBLE_FOLDER)/vars/$(ENVIRONMENT).yml \
		-e system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
		-e nr_license_key=${NR_LICENSE_KEY} \
		-e package_version=${PACKAGE_VERSION} \
		-e fleet_id=${FLEET_ID} \
		-e windows_fleet_id=${WINDOWS_FLEET_ID} \
		$(ONHOST_ANSIBLE_FOLDER)/install_ac_with_basic_config.yml
