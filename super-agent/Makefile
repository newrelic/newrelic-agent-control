.DEFAULT_GOAL := onhost/cargo

CARGO_ARGS ?= ""
CARGO_CMD ?= "test"

.PHONY: onhost/cargo
onhost/cargo:
	@echo "testing-machine-id" > /tmp/machine-id
	TEST_MACHINE_ID_PATH="/tmp/machine-id" cargo $(CARGO_CMD) $(CARGO_ARGS)
