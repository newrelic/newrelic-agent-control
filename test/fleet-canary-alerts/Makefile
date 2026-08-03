# Get the absolute path to the current directory
CURR_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

FLEET_TF_DIR := $(CURR_DIR)terraform

# For production, use the production credentials (both staging and production secrets are available on
# GHA). The alert resources are created in the canary *telemetry* account (NEW_RELIC_ACCOUNT_ID, same
# as the other canary alerts); the condition then queries the fleet-data account cross-account via the
# terraform `data_account_id` (passed in per environment from CI, see DATA_ACCOUNT_ID/FLEET_GUIDS below).
ifeq ($(CANARY_DIR), production)
	NEW_RELIC_API_KEY = $(NEW_RELIC_PROD_API_KEY)
	NEW_RELIC_ACCOUNT_ID = $(NEW_RELIC_PROD_ACCOUNT_ID)
endif

# Validate required env vars. Checks are inlined here (rather than reusing the shared `check_env_var`
# macro) so this Makefile does not redefine names shared with the k8s/onhost canary Makefiles — a
# redefinition would win globally (last include) and change their behavior.
define check_fleet_tf_env_vars
	if [ -z "$$CANARY_DIR" ]; then echo "CANARY_DIR missing"; exit 1; fi; \
	if [ "$$CANARY_DIR" = "staging" ]; then \
		if [ -z "$$NEW_RELIC_ACCOUNT_ID" ]; then echo "NEW_RELIC_ACCOUNT_ID missing"; exit 1; fi; \
		if [ -z "$$NEW_RELIC_API_KEY" ]; then echo "NEW_RELIC_API_KEY missing"; exit 1; fi; \
	elif [ "$$CANARY_DIR" = "production" ]; then \
		if [ -z "$$NEW_RELIC_PROD_ACCOUNT_ID" ]; then echo "NEW_RELIC_PROD_ACCOUNT_ID missing"; exit 1; fi; \
		if [ -z "$$NEW_RELIC_PROD_API_KEY" ]; then echo "NEW_RELIC_PROD_API_KEY missing"; exit 1; fi; \
	else \
		echo "CANARY_DIR must be either 'staging' or 'production'. Got '$$CANARY_DIR'"; exit 1; \
	fi; \
	if [ -z "$$SLACK_WEBHOOK_URL" ]; then echo "SLACK_WEBHOOK_URL missing"; exit 1; fi; \
	if [ -z "$$EMAILS" ]; then echo "EMAILS missing"; exit 1; fi; \
	if [ -z "$$DATA_ACCOUNT_ID" ]; then echo "DATA_ACCOUNT_ID missing"; exit 1; fi; \
	if [ -z "$$FLEET_GUIDS" ]; then echo "FLEET_GUIDS missing"; exit 1; fi
endef

.PHONY: test/fleet-canary-alerts/terraform-plan
test/fleet-canary-alerts/terraform-plan:
	@$(call check_fleet_tf_env_vars)
	@terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) plan \
	-var="account_id=$(NEW_RELIC_ACCOUNT_ID)" -var="api_key=$(NEW_RELIC_API_KEY)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)" -var="emails=$(EMAILS)" \
	-var="data_account_id=$(DATA_ACCOUNT_ID)" -var="fleet_guids=$(FLEET_GUIDS)"

.PHONY: test/fleet-canary-alerts/terraform-apply
test/fleet-canary-alerts/terraform-apply:
	@$(call check_fleet_tf_env_vars)
	@terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) apply -auto-approve \
	-var="account_id=$(NEW_RELIC_ACCOUNT_ID)" -var="api_key=$(NEW_RELIC_API_KEY)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)" -var="emails=$(EMAILS)" \
	-var="data_account_id=$(DATA_ACCOUNT_ID)" -var="fleet_guids=$(FLEET_GUIDS)"

.PHONY: test/fleet-canary-alerts/terraform-destroy
test/fleet-canary-alerts/terraform-destroy:
	@$(call check_fleet_tf_env_vars)
	@terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) destroy -auto-approve \
	-var="account_id=$(NEW_RELIC_ACCOUNT_ID)" -var="api_key=$(NEW_RELIC_API_KEY)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)" -var="emails=$(EMAILS)" \
	-var="data_account_id=$(DATA_ACCOUNT_ID)" -var="fleet_guids=$(FLEET_GUIDS)"
