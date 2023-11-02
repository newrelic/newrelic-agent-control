K3S_IMAGE := 'rancher/k3s:v1.28.2-k3s1'

.PHONY: all
all: test-integration-minikube test-integration-k3s

.PHONY: test-integration-minikube
test-integration-minikube:
	KUBECONFIG='./.kubeconfig-dev' minikube update-context
	cargo test k8s_ -- --ignored

.PHONY: test-integration-k3s
test-integration-k3s:
	docker pull ${K3S_IMAGE}
	K3S_IMAGE=${K3S_IMAGE} cargo test k3s_ -- --ignored
