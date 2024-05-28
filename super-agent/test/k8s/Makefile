.PHONY: all
all: test-integration-minikube

.PHONY: test-integration-minikube
test-integration-minikube:
	KUBECONFIG='./.kubeconfig-dev' minikube update-context
	tilt ci
	LOG_LEVEL=newrelic_super_agent=debug cargo test --features k8s k8s_opamp_add_subagent -- --nocapture --ignored
