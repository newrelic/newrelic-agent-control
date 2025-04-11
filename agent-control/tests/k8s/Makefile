.PHONY: all
all: test-integration-minikube

.PHONY: test-integration-minikube
test-integration-minikube:
	KUBECONFIG='./.kubeconfig-dev' minikube update-context
	tilt ci
	# reducing the number of threads to 1 forces the tests to run sequentially
	cargo test k8s_ -- --nocapture --ignored  --test-threads=1
