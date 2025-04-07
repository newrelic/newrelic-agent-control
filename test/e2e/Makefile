# Include Ansible dependencies common installation strategy
include ../Ansible.common

REPOSITORY_ENDPOINT ?= "https://download.newrelic.com/preview"

# Variables dependent on the New Relic platform region.
OPAMP_ENDPOINT = "https://opamp.staging-service.newrelic.com/v1/opamp"
NEW_RELIC_API_HOST = "https://staging-api.newrelic.com"
ifeq ($(ENVIRONMENT), production)
	OPAMP_ENDPOINT = "https://opamp.service.newrelic.com/v1/opamp"
	NEW_RELIC_API_HOST = "https://api.newrelic.com"
	NR_LICENSE_KEY ?= $(NR_PROD_LICENSE_KEY)
	NEW_RELIC_ACCOUNT_ID ?= $(NEW_RELIC_PROD_ACCOUNT_ID)
	NEW_RELIC_API_KEY ?= $(NEW_RELIC_PROD_API_KEY)
	NR_SYSTEM_IDENTITY_CLIENT_ID ?= $(NR_PROD_SYSTEM_IDENTITY_CLIENT_ID)
	NR_SYSTEM_IDENTITY_PRIVATE_KEY ?= $(NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY)

# The private key is used in the ansible file, it need to be exported
# for the subshell to use the correct value
	export NR_SYSTEM_IDENTITY_PRIVATE_KEY
endif

ANSIBLE_FOLDER := $(CURDIR)/ansible
ANSIBLE_FORKS ?= 5
ANSIBLE_INVENTORY ?= $(ANSIBLE_FOLDER)/inventory.ec2
ANSIBLE_PLAYBOOK ?= test.yaml
LIMIT ?= "testing_hosts_linux"

define check_env_var
	if [ -z "$$$(1)" ]; then \
		echo "$1 missing"; \
		exit 1; \
	fi
endef

define check_env_vars
	$(call check_env_var,ENVIRONMENT)
	if [ "$$ENVIRONMENT" = "staging" ]; then \
		$(call check_env_var,NR_LICENSE_KEY); \
		$(call check_env_var,NEW_RELIC_ACCOUNT_ID); \
		$(call check_env_var,NEW_RELIC_API_KEY); \
		$(call check_env_var,NR_SYSTEM_IDENTITY_CLIENT_ID); \
		$(call check_env_var,NR_SYSTEM_IDENTITY_PRIVATE_KEY); \
	elif [ "$$ENVIRONMENT" = "production" ]; then \
		$(call check_env_var,NR_PROD_LICENSE_KEY); \
		$(call check_env_var,NEW_RELIC_PROD_ACCOUNT_ID); \
		$(call check_env_var,NEW_RELIC_PROD_API_KEY); \
		$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_CLIENT_ID); \
		$(call check_env_var,NR_PROD_SYSTEM_IDENTITY_PRIVATE_KEY); \
	else \
		echo "ENVIRONMENT must be either 'staging' or 'production'"; \
		exit 1; \
	fi
endef

.DEFAULT_GOAL := e2e
.PHONY: e2e
e2e:
	@$(call check_env_vars)
# if you want to hide OK, add ANSIBLE_DISPLAY_OK_HOSTS=NO
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
	@ANSIBLE_DISPLAY_SKIPPED_HOSTS=NO ansible-playbook -f $(ANSIBLE_FORKS) \
 		-i $(ANSIBLE_INVENTORY) \
 		--limit=$(LIMIT) \
 		-e nr_license_key=$(NR_LICENSE_KEY) \
        -e nr_api_key=$(NEW_RELIC_API_KEY) \
		    -e nr_api_host=$(NEW_RELIC_API_HOST) \
        -e nr_account_id=$(NEW_RELIC_ACCOUNT_ID) \
        -e repo_endpoint=$(REPOSITORY_ENDPOINT) \
        -e opamp_endpoint=${OPAMP_ENDPOINT} \
        -e package_version=${PACKAGE_VERSION} \
        -e system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
 		$(ANSIBLE_FOLDER)/$(ANSIBLE_PLAYBOOK)

.PHONY: clean
clean:
	$(MAKE) ansible/clean ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
