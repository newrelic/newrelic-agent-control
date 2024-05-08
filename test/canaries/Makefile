# Include Ansible dependencies common installation strategy
include ../Ansible.common
.DEFAULT_GOAL := packaging

ANSIBLE_FOLDER := $(CURDIR)/ansible
LIMIT ?= "testing_hosts_linux"
ANSIBLE_FORKS ?= 5
ANSIBLE_INVENTORY ?= $(ANSIBLE_FOLDER)/inventory.ec2


.DEFAULT_GOAL := all

.PHONY: all
all: canaries

.PHONY: canaries
canaries:
ifndef NR_LICENSE_KEY_CANARIES
	@echo "NR_LICENSE_KEY_CANARIES variable must be provided for test/canaries"
	exit 1
endif
ifndef NR_OTEL_COLLECTOR_MEMORY_LIMIT
	@echo "NR_OTEL_COLLECTOR_MEMORY_LIMIT variable must be provided for test/canaries"
	exit 1
endif
ifndef NR_OTEL_COLLECTOR_OTLP_ENDPOINT
	@echo "NR_OTEL_COLLECTOR_OTLP_ENDPOINT variable must be provided for test/canaries"
	exit 1
endif

	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
	@ANSIBLE_DISPLAY_SKIPPED_HOSTS=NO ANSIBLE_DISPLAY_OK_HOSTS=NO ansible-playbook -f $(ANSIBLE_FORKS) \
 		-i $(ANSIBLE_INVENTORY) \
 		--limit=$(LIMIT) \
 		-e nr_license_key=$(NR_LICENSE_KEY_CANARIES) \
 		-e nr_otel_collector_license_key=$(NR_LICENSE_KEY_CANARIES) \
 		-e nr_otel_collector_otlp_endpoint=$(NR_OTEL_COLLECTOR_OTLP_ENDPOINT) \
 		-e nr_otel_collector_memory_limit=$(NR_OTEL_COLLECTOR_MEMORY_LIMIT) \
 		$(ANSIBLE_FOLDER)/deploy_canaries.yml
