.DEFAULT_GOAL := gen-proto


SPEC_DIR := ./opamp-spec

gen-proto: gen-rust
.PHONY: gen-proto

# Generate Protobuf Rust files.
.PHONY: gen-rust
gen-rust:
	git submodule update --init
	OUT_DIR=$(CURDIR) cargo run

.PHONY: clean
clean:
	rm -r $(SPEC_DIR)
