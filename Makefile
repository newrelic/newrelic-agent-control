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


ARCH:=arm64
BUILD_MODE:=release

.PHONY: build-super-agent 
# Cross-compilation only works from amd64 host.
build-super-agent:
	@echo "Building with mode: $(BUILD_MODE) and arch: $(ARCH)"
	ARCH=$(ARCH) BUILD_MODE=$(BUILD_MODE) ./build/scripts/build_super_agent.sh

.PHONY: build-dev-image
build-dev-image: build-super-agent
	docker build . -t newrelic-super-agent:dev

.PHONY: tilt-up
tilt-up:
	tilt up ; tilt down
