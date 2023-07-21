.PHONY: third-party-notices
third-party-notices:
	@if ! cargo install --list | grep -q cargo-deny ; then\
        cargo install --locked cargo-deny;\
    fi ;\
    LICENSES=$$(cargo deny --manifest-path ../Cargo.toml list -l crate -f json 2>&1); \
	cargo run -- --dependencies "$$(printf "%s " $$LICENSES)" --output-file "../THIRD_PARTY_NOTICES.md"

.PHONY: third-party-notices-check
third-party-notices-check: third-party-notices
	@git diff --name-only | grep -q "THIRD_PARTY_NOTICES.md" && { echo "Third party notices out of date, please run \"make -C license third-party-notices\" and commit the changes in this PR.";  exit 1; } || exit 0
