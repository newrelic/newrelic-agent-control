# Get the absolute path to the current directory
CURR_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

FLEET_TF_DIR := $(CURR_DIR)terraform

.DEFAULT_GOAL := all

# For production, use the production credentials (both staging and production secrets are available on
# GHA). `account_id` is intentionally NOT passed: each terraform environment defaults to its own
# fleet-data account (staging 12213068 / production 6425865), which is distinct from the canary
# telemetry account used by the other canary configs.
ifeq ($(CANARY_DIR), production)
	NEW_RELIC_API_KEY = $(NEW_RELIC_PROD_API_KEY)
endif

.PHONY: all
all:
	@echo "No default target"

define check_env_var
	@! test -n "$$$(1)" && echo "$1 missing" && exit 1
endef

define check_fleet_tf_env_vars
	$(call check_env_var,CANARY_DIR)
	if [ "$$CANARY_DIR" = "staging" ]; then \
		$(call check_env_var,NEW_RELIC_API_KEY); \
	elif [ "$$CANARY_DIR" = "production" ]; then \
		$(call check_env_var,NEW_RELIC_PROD_API_KEY); \
	else \
		echo "CANARY_DIR must be either 'staging' or 'production'. Got '$(CANARY_DIR)'"; \
		exit 1; \
	fi
	$(call check_env_var,SLACK_WEBHOOK_URL)
	$(call check_env_var,EMAILS)
endef

.PHONY: test/fleet-canaries/terraform-plan
test/fleet-canaries/terraform-plan:
	@$(call check_fleet_tf_env_vars)
	@terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) plan \
	-var="api_key=$(NEW_RELIC_API_KEY)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)" -var="emails=$(EMAILS)"

.PHONY: test/fleet-canaries/terraform-apply
test/fleet-canaries/terraform-apply:
	@$(call check_fleet_tf_env_vars)
	@terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(FLEET_TF_DIR)/$(CANARY_DIR) apply -auto-approve \
	-var="api_key=$(NEW_RELIC_API_KEY)" -var="slack_webhook_url=$(SLACK_WEBHOOK_URL)" -var="emails=$(EMAILS)"
