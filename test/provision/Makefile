# Include Ansible dependencies common installation strategy
include ../Ansible.common

TERRAFORM_DIR := ./terraform
ANSIBLE_FOLDER := ./terraform
EC2_FILTERS ?= [\"ubuntu\",\"centos7\",\"centos8\",\"centos-stream\",\"sles-15.3\",\"sles-15.4\",\"sles-15.5\",\"redhat\",\"debian-bullseye\",\"debian-bookworm\",\"al\"]

.DEFAULT_GOAL := provision

.PHONY: terraform/backend
terraform/backend:
ifndef TAG_OR_UNIQUE_NAME
	$(error TAG_OR_UNIQUE_NAME is undefined)
endif
	sed "s/TAG_OR_UNIQUE_NAME/${TAG_OR_UNIQUE_NAME}/g" "$(TERRAFORM_DIR)/terraform.backend.tf.dist" > "$(TERRAFORM_DIR)/terraform.backend.tf"
	sed -e "s/TAG_OR_UNIQUE_NAME/${TAG_OR_UNIQUE_NAME}/g" -e "s/EC2_FILTERS_VALUE/$(EC2_FILTERS)/g" "$(TERRAFORM_DIR)/caos-linux.auto.tfvars.dist" > "$(TERRAFORM_DIR)/caos.auto.tfvars"

.PHONY: provision
provision: terraform/backend
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER) ANSIBLE_ENV_SCRIPT=$(ANSIBLE_FOLDER)/set_ansible_env.sh
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure && \
	terraform -chdir=$(TERRAFORM_DIR) apply -auto-approve

.PHONY: clean
clean: terraform/backend ansible/clean
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure && \
    terraform -chdir=$(TERRAFORM_DIR) destroy -auto-approve && \
	rm "$(TERRAFORM_DIR)/terraform.backend.tf" "$(TERRAFORM_DIR)/caos.auto.tfvars"
	$(MAKE) ansible/clean ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
