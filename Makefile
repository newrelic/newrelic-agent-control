##########################################
# 		     Dynamic targets 			 #
##########################################
# Exclude current and hidden directories
FIND_PATH = . -mindepth 2 -not -path '*/\.*'
# Define the list of subdirectories that contain a Makefile
SUBDIRS := $(patsubst ./%/Makefile,%,$(shell find $(FIND_PATH) -name Makefile))
TARGETS := $(SUBDIRS)

.PHONY: all $(TARGETS) clean $(addsuffix -clean,$(TARGETS)) help

$(TARGETS):
	$(MAKE) -C $@

clean: $(addsuffix -clean,$(SUBDIRS))

$(addsuffix -clean,$(TARGETS)):
	$(MAKE) -C $(patsubst %-clean,%,$@) clean


##########################################
# 		     Static targets 			 #
##########################################
#include build/embedded/Makefile

help:
	@echo "## Available targets:"
	@echo $(TARGETS)
	@echo "## Available clean targets:"
	@echo $(addsuffix -clean,$(TARGETS))


ARCH ?= arm64
BUILD_MODE ?= release

.PHONY: build-agent-control 
build-agent-control:
	@echo "Building with mode: $(BUILD_MODE) and arch: $(ARCH)"
	ARCH=$(ARCH) BUILD_MODE=$(BUILD_MODE) BIN="newrelic-agent-control" PKG="newrelic_agent_control" ./build/scripts/build_binary.sh

# Cross-compilation only works from amd64 host.
build-config-migrate:
	@echo "Building with mode: $(BUILD_MODE) and arch: $(ARCH)"
	ARCH=$(ARCH) BUILD_MODE=$(BUILD_MODE) BIN="newrelic-config-migrate" PKG="newrelic_agent_control" BUILD_FEATURE=onhost ./build/scripts/build_binary.sh

.PHONY: tilt-up
tilt-up:
	tilt up ; tilt down

COVERAGE_OUT_FORMAT ?= lcov
COVERAGE_OUT_FILEPATH ?= coverage/lcov.info
coverage: llvm-cov
	@echo "Generating coverage report..."
	@cargo llvm-cov clean --workspace
	@cargo llvm-cov --no-report --locked --features=k8s --workspace --exclude config-migrate --lib
	@cargo llvm-cov --no-report --locked --features=onhost --lib
	@mkdir -p coverage
	@cargo llvm-cov report --$(COVERAGE_OUT_FORMAT) --output-path $(COVERAGE_OUT_FILEPATH)

.PHONY: llvm-cov
llvm-cov:
	@echo "Checking if llvm-cov is installed..."
	@which cargo-llvm-cov || cargo install cargo-llvm-cov --locked