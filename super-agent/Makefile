.DEFAULT_GOAL := onhost/cargo

CARGO_ARGS ?= ""
CARGO_CMD ?= "test"

.PHONY: onhost/cargo
onhost/cargo:
	@echo "testing-machine-id" > /tmp/machine-id
	TEST_MACHINE_ID_PATH="/tmp/machine-id" TEST_IPV4_METADATA_ENDPOINT="http://127.0.0.1:4343/testing_metadata_endpoint" cargo $(CARGO_CMD) $(CARGO_ARGS)
