LICENSES := $(shell cargo deny --manifest-path ../Cargo.toml list -l crate -f json 2>&1)

third-party-notices:
	@cargo run -- --dependencies '$(LICENSES)' --output-file '../THIRD_PARTY_NOTICES.md'