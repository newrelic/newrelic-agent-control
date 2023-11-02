# k8s examples

Temporal examples for development purposes.

The examples require a k8s cluster. They use `.kubeconfig` and the default context, so beware of
your context before executing. They were tested using minikube.

```bash
# start minikube
$ minikube start
# execute examples (check their code for details)
$ RUST_LOG=info cargo run --example k8s-dynamic-reflectors --features k8s
$ RUST_LOG=info cargo run --example k8s-dynamic-crs-api --features k8s
```
