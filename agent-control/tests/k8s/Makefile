.PHONY: all
all: test-integration-minikube

.PHONY: test-integration-minikube
test-integration-minikube:
	KUBECONFIG='./.kubeconfig-dev' minikube update-context
	tilt ci
	# reducing the number of threads to 1 forces the tests to run sequentially
	LOG_LEVEL=newrelic_agent_control=debug cargo test --features k8s k8s_ -- --nocapture --ignored  --test-threads=1
