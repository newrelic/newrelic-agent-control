##########################################
# 		     Dynamic targets 			 #
##########################################
# Exclude current and hidden directories
FIND_PATH = . -mindepth 2 -not -path '*/\.*'
# Define the list of subdirectories that contain a Makefile
SUBDIRS := $(patsubst ./%/Makefile,%,$(shell find $(FIND_PATH) -name Makefile))
TARGETS := $(SUBDIRS)

.PHONY: all $(TARGETS) help

$(TARGETS):
	$(MAKE) -C $@

##########################################
# 		     Static targets 			 #
##########################################
include test/k8s-canaries/Makefile
include test/onhost-canaries/Makefile
include test/fleet-canary-alerts/Makefile

help:
	@echo "## Available targets:"
	@echo $(TARGETS)

ARCH ?= arm64
BUILD_MODE ?= release

build-%:
	@echo "Building $* with mode: $(BUILD_MODE), bin $(*) and arch: $(ARCH)"
	ARCH=$(ARCH) BUILD_MODE=$(BUILD_MODE) BIN="newrelic-$(*)" PKG="newrelic_agent_control" ./build/scripts/build_binary.sh

.PHONY: tilt-up
tilt-up:
	tilt up ; tilt down

##########################################
# 		     Image targets 			 #
##########################################
DOCKER_IMAGE_NAME_AGENT_CONTROL ?= newrelic/newrelic-agent-control
DOCKER_IMAGE_NAME_AGENT_CONTROL_CLI ?= newrelic/newrelic-agent-control-cli
DOCKER_PLATFORMS ?= linux/$(ARCH)
IMAGE_TAG ?= local
# PUSH pushes to the registry; LOAD loads into the local docker daemon instead
# (single platform only). ATTEST attaches provenance/SBOM attestations, which
# requires PUSH.
PUSH ?= false
LOAD ?= true
ATTEST ?= false

DOCKERFILE_AGENT_CONTROL := Dockerfiles/Dockerfile_agent_control
DOCKERFILE_AGENT_CONTROL_CLI := Dockerfiles/Dockerfile_agent_control_cli

ATTEST_FLAGS := --attest type=provenance,mode=max --attest type=sbom

DOCKER_BUILD_OUTPUT_FLAG = $(if $(filter true,$(PUSH)),--push,$(if $(filter true,$(LOAD)),--load))
DOCKER_BUILD_ATTEST_FLAGS = $(if $(filter true,$(ATTEST)),$(ATTEST_FLAGS))

# Assumes the binaries for $(DOCKER_PLATFORMS) already exist under ./bin (e.g.
# `make build-agent-control-k8s`). No build prerequisite on purpose, so CI can
# call these directly without triggering a second, differently-configured
# build. Use `make image` for a one-command local build.
.PHONY: image/agent-control
image/agent-control:
	docker buildx build \
		--platform=$(DOCKER_PLATFORMS) \
		-t $(DOCKER_IMAGE_NAME_AGENT_CONTROL):$(IMAGE_TAG) \
		--file $(DOCKERFILE_AGENT_CONTROL) \
		$(DOCKER_BUILD_OUTPUT_FLAG) \
		$(DOCKER_BUILD_ATTEST_FLAGS) \
		.

.PHONY: image/agent-control-cli
image/agent-control-cli:
	docker buildx build \
		--platform=$(DOCKER_PLATFORMS) \
		-t $(DOCKER_IMAGE_NAME_AGENT_CONTROL_CLI):$(IMAGE_TAG) \
		--file $(DOCKERFILE_AGENT_CONTROL_CLI) \
		$(DOCKER_BUILD_OUTPUT_FLAG) \
		$(DOCKER_BUILD_ATTEST_FLAGS) \
		.

.PHONY: image
image:
	$(MAKE) build-agent-control-k8s
	$(MAKE) build-agent-control-k8s-cli
	$(MAKE) image/agent-control
	$(MAKE) image/agent-control-cli

COVERAGE_OUT_FORMAT ?= lcov
COVERAGE_OUT_FILEPATH ?= coverage/lcov.info
coverage: llvm-cov
	@echo "Generating coverage report..."
	@cargo llvm-cov clean --workspace
	@cargo llvm-cov --no-report --locked --all-features --lib
	@mkdir -p coverage
	@cargo llvm-cov report --$(COVERAGE_OUT_FORMAT) --output-path $(COVERAGE_OUT_FILEPATH)

.PHONY: llvm-cov
llvm-cov:
	@echo "Checking if llvm-cov is installed..."
	@which cargo-llvm-cov || cargo install cargo-llvm-cov --locked

# Build rustdoc for the whole workspace. RUSTDOCFLAGS mirrors CI so broken doc
# links / invalid HTML fail locally too. Use `make doc-open` to open in a browser.
RUSTDOCFLAGS_DOC ?= --cfg docsrs -D warnings
.PHONY: doc
doc:
	@echo "Building workspace documentation..."
	@RUSTDOCFLAGS="$(RUSTDOCFLAGS_DOC)" cargo doc --no-deps --workspace

.PHONY: doc-open
doc-open:
	@echo "Building and opening workspace documentation..."
	@RUSTDOCFLAGS="$(RUSTDOCFLAGS_DOC)" cargo doc --no-deps --workspace --open

.PHONY: third-party-notices
third-party-notices:
	@echo "Checking third-party licenses..."
	@(cargo install --list | grep cargo-deny) || cargo install cargo-deny --locked
	@(cargo install --list | grep rust-licenses-noticer) || cargo install --git https://github.com/newrelic/rust-licenses-noticer.git --locked
	@LICENSES=$$(cargo deny --all-features --locked --manifest-path ./Cargo.toml list -l crate -f json 2>&1); \
    $$HOME/.cargo/bin/rust-licenses-noticer --dependencies "$$(printf "%s " $$LICENSES)" --template-file "./THIRD_PARTY_NOTICES.md.tmpl" --output-file "./THIRD_PARTY_NOTICES.md"

.PHONY: third-party-notices-check
third-party-notices-check: third-party-notices
	@git diff --name-only | grep -q "THIRD_PARTY_NOTICES.md" && { echo "Third party notices out of date, please commit the changes to the THIRD_PARTY_NOTICES.md file.";  exit 1; } || exit 0

# rt-update-changelog runs the release-toolkit run.sh script by piping it into bash to update the CHANGELOG.md.
# It also passes down to the script all the flags added to the make target. To check all the accepted flags,
# see: https://github.com/newrelic/release-toolkit/blob/main/contrib/ohi-release-notes/run.sh
#  e.g. `make rt-update-changelog -- -v`
rt-update-changelog:
	curl "https://raw.githubusercontent.com/newrelic/release-toolkit/v1/contrib/ohi-release-notes/run.sh" | bash -s -- $(filter-out $@,$(MAKECMDGOALS))
