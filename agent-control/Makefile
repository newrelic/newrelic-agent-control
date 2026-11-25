.DEFAULT_GOAL := onhost/cargo

CARGO_ARGS ?= ""
CARGO_CMD ?= nextest run
PACKAGE_SA = --locked --package newrelic_agent_control

TEST_FIXTURES = TEST_MACHINE_ID_PATH="/tmp/machine-id"

.PHONY: onhost/cargo
onhost/cargo:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(CARGO_ARGS)

.PHONY: test/onhost
test/onhost:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --lib

.PHONY: test/onhost/integration
test/onhost/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --test integration_tests

.PHONY: test/onhost/root/integration
test/onhost/root/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --test integration_tests --run-ignored only _as_root -- --skip no_root --skip k8s

.PHONY: test/onhost/oci-registry/integration
test/onhost/oci-registry/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	@../tools/oci-registry.sh install
	@../tools/oci-registry.sh clean
	@../tools/oci-registry.sh run > /dev/null 2>&1 &
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --test integration_tests --run-ignored only _with_oci_registry -- --skip no_root --skip k8s
	@../tools/oci-registry.sh killall

.PHONY: test/onhost/oci-auth-registry/integration
test/onhost/oci-auth-registry/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	@../tools/oci-registry.sh install --basic-auth
	@../tools/oci-registry.sh clean
	@../tools/oci-registry.sh run --basic-auth > /dev/null 2>&1 &
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --test integration_tests --run-ignored only _with_auth_oci_registry -- --skip no_root --skip k8s
	@../tools/oci-registry.sh killall

# K8S

.PHONY: test/k8s
test/k8s:
	cargo $(CARGO_CMD) $(PACKAGE_SA) --all-targets -- --skip on_host

# K8S integration tests without Flux - runs directly without Tilt
.PHONY: test/k8s/integration-without-flux
test/k8s/integration-without-flux:
	@echo "==> Updating kubeconfig for minikube..."
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	@echo "==> Running integration tests without Flux..."
	cargo $(CARGO_CMD) --run-ignored only --test-threads 10 k8s::scenarios::without_flux

# K8S integration tests with Flux
.PHONY: test/k8s/integration
test/k8s/integration: test/k8s/integration-part1 test/k8s/integration-part2

.PHONY: test/k8s/integration-part1
test/k8s/integration-part1:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	ENABLE_VAULT=true  tilt ci --file ./tests/k8s/Tiltfile
	cargo $(CARGO_CMD) --run-ignored only --test-threads 10 k8s::scenarios -- --skip k8s::scenarios::install_agent_type
	cargo $(CARGO_CMD) --run-ignored only --test-threads 10 k8s::agent_control_cli

# Flux upgrade tests require isolated test environments with Flux installed.
# This installs an initial Flux instance with CRDs that only reconciles the default namespace,
# allowing each test to create its own namespace with a separate Flux instance.
.PHONY: test/k8s/integration-flux-self-update
test/k8s/integration-flux-self-update:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	WATCH_ALL_NAMESPACES=false tilt ci --file ./tests/k8s/Tiltfile
	cargo $(CARGO_CMD) --run-ignored only --test-threads 10 k8s::flux_self_update

.PHONY: test/k8s/integration-part2
test/k8s/integration-part2:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	tilt ci --file ./tests/k8s/Tiltfile
	# We use this approach to split the k8s tests into two parts. We need to update it if this takes too long to run.
	cargo $(CARGO_CMD) --run-ignored only --test-threads 10 k8s_ -- --skip k8s::scenarios --skip k8s::agent_control_cli --skip k8s::flux_self_update

.PHONY: test/k8s/integration-oci-registry
test/k8s/integration-oci-registry:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	@../tools/oci-registry.sh install
	@../tools/oci-registry.sh run > /dev/null 2>&1 &
	cargo $(CARGO_CMD) --run-ignored only --test-threads 10 k8s::scenarios::install_agent_type -- --skip on_host
	@../tools/oci-registry.sh killall
