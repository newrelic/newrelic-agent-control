# k8s Integration tests 

Requirements:
- [Install minikube](https://minikube.sigs.k8s.io/docs/start/)

Run:
```sh
minikube start
make test/k8s test-integration-k8s
```

Notes:
- Tests that required k8s cluster must be prefixed `k8s_` as a convention to filter them.
- k8s clients must be configured to use a local file `.kubeconfig-dev`.
- Same cluster will be used to run all tests so care must be taken on isolation.
