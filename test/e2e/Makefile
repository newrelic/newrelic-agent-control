# Include Ansible dependencies common installation strategy
include ../Ansible.common
.DEFAULT_GOAL := e2e

ANSIBLE_FOLDER := $(CURDIR)/ansible
LIMIT ?= "testing_hosts_linux"

.PHONY: e2e
e2e:
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
	@ANSIBLE_DISPLAY_SKIPPED_HOSTS=NO ANSIBLE_DISPLAY_OK_HOSTS=NO ansible-playbook -f $(ANSIBLE_FORKS) \
 		-i $(ANSIBLE_INVENTORY) \
 		--limit=$(LIMIT) \
 		$(ANSIBLE_FOLDER)/hello.yaml

.PHONY: clean
clean:
	$(MAKE) ansible/clean ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
