# Include Ansible dependencies common installation strategy
include ../Ansible.common

TERRAFORM_DIR := ./terraform
ANSIBLE_FOLDER := ./terraform

.DEFAULT_GOAL := provision

.PHONY: terraform/backend
terraform/backend:
ifndef TAG_OR_UNIQUE_NAME
	$(error TAG_OR_UNIQUE_NAME is undefined)
endif
	sed "s/TAG_OR_UNIQUE_NAME/${TAG_OR_UNIQUE_NAME}/g" "$(TERRAFORM_DIR)/terraform.backend.tf.dist" > "$(TERRAFORM_DIR)/terraform.backend.tf"
	sed "s/TAG_OR_UNIQUE_NAME/${TAG_OR_UNIQUE_NAME}/g" "$(TERRAFORM_DIR)/caos-linux.auto.tfvars.dist" > "$(TERRAFORM_DIR)/caos.auto.tfvars"
	EC2_FILTERS := $(or $(EC2_FILTERS),[""])
	sed "s/EC2_FILTERS/${EC2_FILTERS}/g" "$(TERRAFORM_DIR)/caos-linux.auto.tfvars.dist" > "$(TERRAFORM_DIR)/caos.auto.tfvars"

.PHONY: provision
provision: terraform/backend
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure && \
	terraform -chdir=$(TERRAFORM_DIR) apply -auto-approve

.PHONY: clean
clean: terraform/backend ansible/clean
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure && \
    terraform -chdir=$(TERRAFORM_DIR) destroy -auto-approve && \
	rm "$(TERRAFORM_DIR)/terraform.backend.tf" "$(TERRAFORM_DIR)/caos.auto.tfvars"
	$(MAKE) ansible/clean ANSIBLE_FOLDER=$(ANSIBLE_FOLDER)
