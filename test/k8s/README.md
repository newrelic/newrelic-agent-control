# k8s Integration tests 

Requirements:
- Docker
- [Install minikube](https://minikube.sigs.k8s.io/docs/start/)

On the repo root directory Run:
```sh
minikube start
make test/k8s
```

Notes:
- Tests that require a k8s cluster must be prefixed `k8s_` as a convention to filter them.
- The `KUBECONFIG` env var is override on each test execution and points to the dev cluster, so k8s clients used in the test can be configured to use the `KUBECONFIG` environment.
- There are currently two kind of tests, one using the same cluster but creating namespaces and the other is creating a cluster for the tests which is destroyed when the test finishes. The first one si much faster but less isolated.
