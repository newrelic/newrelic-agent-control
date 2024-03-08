# Include Ansible dependencies common installation strategy
include ../Ansible.common
.DEFAULT_GOAL := e2e

ANSIBLE_FOLDER := $(CURDIR)/ansible
ANSIBLE_FORKS ?= 5
ANSIBLE_INVENTORY ?= $(ANSIBLE_FOLDER)/inventory.ec2
ANSIBLE_PLAYBOOK ?= test.yaml
LIMIT ?= "testing_hosts_linux"
REPOSITORY_ENDPOINT ?= "https://download.newrelic.com/preview"
NR_OTEL_COLLECTOR_OTLP_ENDPOINT ?= "staging-otlp.nr-data.net:4317"
OPAMP_ENDPOINT ?= "https://opamp.staging-service.newrelic.com/v1/opamp"

.PHONY: e2e
e2e:
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY variable must be provided for test/packaging"
	exit 1
endif
ifndef NEW_RELIC_ACCOUNT_ID
	@echo "NEW_RELIC_ACCOUNT_ID variable must be provided for test/packaging"
	exit 1
endif
ifndef NEW_RELIC_API_KEY
	@echo "NEW_RELIC_API_KEY variable must be provided for test/packaging"
	exit 1
endif

# if you want to hide OK, add ANSIBLE_DISPLAY_OK_HOSTS=NO
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
	@ANSIBLE_DISPLAY_SKIPPED_HOSTS=NO ansible-playbook -f $(ANSIBLE_FORKS) \
 		-i $(ANSIBLE_INVENTORY) \
 		--limit=$(LIMIT) \
 		-e nr_license_key=$(NR_LICENSE_KEY) \
        -e nr_api_key=$(NEW_RELIC_API_KEY) \
        -e nr_account_id=$(NEW_RELIC_ACCOUNT_ID) \
        -e repo_endpoint=$(REPOSITORY_ENDPOINT) \
        -e nr_otel_collector_otlp_endpoint=$(NR_OTEL_COLLECTOR_OTLP_ENDPOINT) \
        -e opamp_endpoint=${OPAMP_ENDPOINT} \
 		$(ANSIBLE_FOLDER)/$(ANSIBLE_PLAYBOOK)

.PHONY: clean
clean:
	$(MAKE) ansible/clean ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
