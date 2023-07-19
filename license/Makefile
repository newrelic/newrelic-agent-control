LICENSES := $(shell cargo deny --manifest-path ../Cargo.toml list -l crate -f json 2>&1)

.PHONY: third-party-notices
third-party-notices:
	@cargo run -- --dependencies '$(LICENSES)' --output-file '../THIRD_PARTY_NOTICES.md'

.PHONY: third-party-notices-check
third-party-notices-check: third-party-notices
	@git diff --name-only | grep -q "THIRD_PARTY_NOTICES.md" && { echo "Third party notices out of date, please run \"make -C license third-party-noticess\" and commit the changes in this PR.";  exit 1; } || exit 0
