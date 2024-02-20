.DEFAULT_GOAL := onhost/integration

CARGO_FLAGS ?= ""

.PHONY: onhost/integration
onhost/integration:
	TEST_IPV4_METADATA_ENDPOINT="http://127.0.0.1:4343/testing_metadata_endpoint" cargo test $(CARGO_FLAGS)
