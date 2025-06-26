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
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --lib

.PHONY: test/onhost/integration
test/onhost/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=multiple-instances --test integration_tests -- --test-threads=1

.PHONY: test/onhost/root
test/onhost/root:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --lib _as_root -- --ignored --skip no_root --skip k8s

.PHONY: test/onhost/root/integration
test/onhost/root/integration:
	@echo "testing-machine-id" > /tmp/machine-id
	$(TEST_FIXTURES) cargo $(CARGO_CMD) $(PACKAGE_SA) --features=multiple-instances --test integration_tests _as_root -- --ignored --skip no_root --skip k8s --test-threads=1

# K8S

.PHONY: test/k8s
test/k8s:
	cargo $(CARGO_CMD) $(PACKAGE_SA) --all-targets -- --skip on_host

.PHONY: test/k8s/integration
test/k8s/integration: test/k8s/integration-part1 test/k8s/integration-part2



.PHONY: test/k8s/integration-part1
test/k8s/integration-part1:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	tilt ci --file ./tests/k8s/Tiltfile
	# reducing the number of threads to 1 forces the tests to run sequentially
	cargo test k8s::scenarios -- --nocapture --ignored  --test-threads=1
	cargo test k8s::agent_control_cli -- --nocapture --ignored  --test-threads=1

.PHONY: test/k8s/integration-part2
test/k8s/integration-part2:
	KUBECONFIG='./tests/k8s/.kubeconfig-dev' minikube update-context
	tilt ci --file ./tests/k8s/Tiltfile
	# reducing the number of threads to 1 forces the tests to run sequentially
	# We use this approach to split the k8s tests into two parts. We need to update it if this takes too long to run.
	cargo test k8s_ --  --skip k8s::scenarios --skip k8s::agent_control_cli --nocapture --ignored  --test-threads=1
