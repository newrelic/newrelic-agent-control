# Include Ansible dependencies common installation strategy
include ../Ansible.common
.DEFAULT_GOAL := e2e

ANSIBLE_FOLDER := $(CURDIR)/ansible
ANSIBLE_FORKS ?= 5
ANSIBLE_INVENTORY ?= $(ANSIBLE_FOLDER)/inventory.ec2
ANSIBLE_PLAYBOOK ?= basic.yaml
LIMIT ?= "testing_hosts_linux"

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

	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
	@ANSIBLE_DISPLAY_SKIPPED_HOSTS=NO ANSIBLE_DISPLAY_OK_HOSTS=NO ansible-playbook -f $(ANSIBLE_FORKS) \
 		-i $(ANSIBLE_INVENTORY) \
 		--limit=$(LIMIT) \
 		-e nr_license_key=$(NR_LICENSE_KEY) \
        -e nr_api_key=$(NEW_RELIC_API_KEY) \
        -e nr_account_id=$(NEW_RELIC_ACCOUNT_ID) \
 		$(ANSIBLE_FOLDER)/$(ANSIBLE_PLAYBOOK)

.PHONY: clean
clean:
	$(MAKE) ansible/clean ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
