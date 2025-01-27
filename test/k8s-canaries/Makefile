TERRAFORM_DIR := ./terraform

.PHONY: sync
sync:
ifndef CANARY_DIR
	@echo "CANARY_DIR variable must be provided to know which canary to sync"
	exit 1
endif
	terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) init && \
	terraform -chdir=$(TERRAFORM_DIR)/$(CANARY_DIR) apply -auto-approve
