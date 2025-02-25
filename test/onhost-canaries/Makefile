# Include Ansible dependencies common installation strategy
include test/Ansible.common

TERRAFORM_DIR := ./terraform
ANSIBLE_FOLDER := ./ansible

.DEFAULT_GOAL := all

.PHONY: all
all:
	@echo "No default target"

.PHONY: test/onhost-canaries/terraform-plan
test/onhost-canaries/terraform-plan:
ifndef ENVIRONMENT
	@echo "ENVIRONMENT variable must be provided to know which canary to test/onhost-canaries/terraform-plan"
	exit 1
endif
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY variable must be provided for test/onhost-canaries/terraform-plan"
	exit 1
endif
ifndef NR_SYSTEM_IDENTITY_CLIENT_ID
	@echo "NR_SYSTEM_IDENTITY_CLIENT_ID variable must be provided for test/onhost-canaries/terraform-plan"
	exit 1
endif 
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
	TF_VAR_license_key=${NR_LICENSE_KEY} \
	terraform -chdir=$(TERRAFORM_DIR) plan -var-file "environments/$(ENVIRONMENT)/inputs.tfvars" 

.PHONY: test/onhost-canaries/terraform-apply
test/onhost-canaries/terraform-apply:
ifndef ENVIRONMENT
	@echo "ENVIRONMENT variable must be provided to know which canary to test/onhost-canaries/terraform-apply"
	exit 1
endif
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY variable must be provided for test/onhost-canaries/terraform-apply"
	exit 1
endif
ifndef NR_SYSTEM_IDENTITY_CLIENT_ID
	@echo "NR_SYSTEM_IDENTITY_CLIENT_ID variable must be provided for test/onhost-canaries/terraform-apply"
	exit 1
endif 
ifndef NR_SYSTEM_IDENTITY_PRIVATE_KEY
	@echo "NR_SYSTEM_IDENTITY_PRIVATE_KEY variable must be provided for test/onhost-canaries/terraform-apply"
	exit 1
endif
	$(MAKE) ansible/dependencies ANSIBLE_FOLDER=$(ANSIBLE_FOLDER) ANSIBLE_ENV_SCRIPT=$(ANSIBLE_FOLDER)/set_ansible_env.sh
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
	TF_VAR_license_key=${NR_LICENSE_KEY} \
	terraform -chdir=$(TERRAFORM_DIR) apply -auto-approve -var-file "environments/$(ENVIRONMENT)/inputs.tfvars" 

.PHONY: test/onhost-canaries/terraform-destroy
test/onhost-canaries/terraform-destroy:
ifndef ENVIRONMENT
	@echo "ENVIRONMENT variable must be provided to know which canary to test/onhost-canaries/terraform-destroy"
	exit 1
endif
ifndef NR_LICENSE_KEY
	@echo "NR_LICENSE_KEY variable must be provided for test/onhost-canaries/terraform-destroy"
	exit 1
endif
ifndef NR_SYSTEM_IDENTITY_CLIENT_ID
	@echo "NR_SYSTEM_IDENTITY_CLIENT_ID variable must be provided for test/onhost-canaries/terraform-destroy"
	exit 1
endif 
ifndef NR_SYSTEM_IDENTITY_PRIVATE_KEY
	@echo "NR_SYSTEM_IDENTITY_PRIVATE_KEY variable must be provided for test/onhost-canaries/terraform-destroy"
	exit 1
endif
	terraform -chdir=$(TERRAFORM_DIR) init -reconfigure -backend-config=environments/$(ENVIRONMENT)/backend.conf && \
	TF_VAR_system_identity_client_id=${NR_SYSTEM_IDENTITY_CLIENT_ID} \
	TF_VAR_license_key=${NR_LICENSE_KEY} \
    terraform -chdir=$(TERRAFORM_DIR) destroy -auto-approve -var-file "environments/$(ENVIRONMENT)/inputs.tfvars"
