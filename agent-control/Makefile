.DEFAULT_GOAL := onhost/cargo

CARGO_ARGS ?= ""
CARGO_CMD ?= "test"
PACKAGE_SA = --locked --package newrelic_agent_control

TEST_FIXTURES = TEST_MACHINE_ID_PATH="/tmp/machine-id"

.PHONY: onhost/cargo
onhost/cargo:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(CARGO_ARGS)

.PHONY: test/doc
test/doc:
	cargo $(CARGO_CMD) --locked --doc

.PHONY: test/onhost
test/onhost:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --lib

.PHONY: test/onhost/integration
test/onhost/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=multiple-instances --test integration_tests -- --test-threads=1 # single-threaded for log readability

.PHONY: test/onhost/root/integration
test/onhost/root/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=multiple-instances --test integration_tests _as_root -- --ignored --skip no_root --skip k8s

.PHONY: test/exclude-agent-control-crate
test/exclude-agent-control-crate:
	cargo $(CARGO_CMD) --workspace --exclude 'newrelic_agent_control' --all-targets

.PHONY: test/k8s
test/k8s:
	cargo $(CARGO_CMD) $(PACKAGE_SA) --all-targets -- --skip on_host

.PHONY: test/k8s/integration
test/k8s/integration: test/k8s/integration-part1 test/k8s/integration-part2

.PHONY: test/k8s/integration-part1
test/k8s/integration-part1:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	ENABLE_VAULT=true  tilt ci --file ./tests/k8s/Tiltfile
	# reduce test threads to 1 to see sequential logs
	cargo test k8s::scenarios -- --nocapture --ignored  --test-threads=10
	cargo test k8s::agent_control_cli -- --nocapture --ignored  --test-threads=10

# Flux upgrade tests require isolated test environments with Flux installed.
# This installs an initial Flux instance with CRDs that only reconciles the default namespace,
# allowing each test to create its own namespace with a separate Flux instance.
.PHONY: test/k8s/integration-flux-self-update
test/k8s/integration-flux-self-update:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	WATCH_ALL_NAMESPACES=false tilt ci --file ./tests/k8s/Tiltfile
	# reduce test threads to 1 to see sequential logs
	cargo test k8s::flux_self_update -- --nocapture --ignored  --test-threads=10

.PHONY: test/k8s/integration-part2
test/k8s/integration-part2:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	tilt ci --file ./tests/k8s/Tiltfile
	# reduce test threads to 1 to see sequential logs
	# We use this approach to split the k8s tests into two parts. We need to update it if this takes too long to run.
	cargo test k8s_ --  --skip k8s::scenarios --skip k8s::agent_control_cli --skip k8s::flux_self_update --nocapture --ignored  --test-threads=10
