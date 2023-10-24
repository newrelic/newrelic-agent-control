K3S_IMAGE := 'rancher/k3s:v1.28.2-k3s1'
.PHONY: test-integration-k8s
test-integration-k8s:
	KUBECONFIG='./.kubeconfig-dev' minikube update-context
	docker pull ${K3S_IMAGE}
	K3S_IMAGE=${K3S_IMAGE} cargo test k8s_ -- --ignored
