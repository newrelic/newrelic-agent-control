.DEFAULT_GOAL := onhost/cargo

CARGO_ARGS ?= ""
CARGO_CMD ?= "test"
PACKAGE_SA = --locked --package newrelic_agent_control

TEST_FIXTURES = TEST_MACHINE_ID_PATH="/tmp/machine-id"

.PHONY: onhost/cargo
onhost/cargo:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(CARGO_ARGS)

.PHONY: test/onhost
test/onhost:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=onhost --lib -- --skip as_root

.PHONY: test/onhost/integration
test/onhost/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=onhost,multiple-instances --test integration_tests -- --skip as_root --test-threads=1

.PHONY: test/onhost/root
test/onhost/root:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=onhost --lib -- --skip no_root

.PHONY: test/onhost/root/integration
test/onhost/root/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=onhost,multiple-instances --test integration_tests -- --skip no_root --test-threads=1

# K8S

.PHONY: test/k8s
test/k8s:
	cargo $(CARGO_CMD) $(PACKAGE_SA) --features=k8s --all-targets
.PHONY: test/k8s/integration
test/k8s/integration:
	$(MAKE) -C tests/k8s
