# k8s Integration tests

Requirements:
- Docker
- [Install minikube](https://minikube.sigs.k8s.io/docs/start/)

On the repo root directory Run:
```sh
minikube start
make agent-control/tests/k8s
```

Notes:
- Tests that require a k8s cluster must be prefixed `k8s_` as a convention to filter them.
- The `KUBECONFIG` env var is overridden on each test execution and points to the dev cluster, so k8s clients used in the test can be configured to use the `KUBECONFIG` environment.
- Tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop, so `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]` needs to be added
- Any test requiring a running k8s cluster should be implemented this way: unit-tests in the agent-control create define the k8s client as a mock using [mockall::double](https://docs.rs/mockall_double/latest/mockall_double/). Therefore, using the _real_ implementation can be problematic. This also means that the mocks defined in the agent-control create cannot be used in integration test (if a particular component mock is needed we need to either re-implement it in the integration test module or expose them in the agent-control crate, outside the `test` feature flag).

## sync / async integration tests

Some tests use the `SyncK8sClient` which encapsulates calls to `runtime.block_on` to offer a synchronous interface.
When this client is used, `#[tokio::test]` cannot be used because `runtime.block_on` would be executing in a tokio
runtime context, leading to a panic:

```
'Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.'
```

This kind of test, needs to be implemented as a regular test, and any asynchronous call needs to be in a `runtime.block_on`
block. Example:

```rust
#[test]
#[ignore = "needs k8s cluster"]
fn test_whatever() {
    let runtime = super::common::tokio_runtime();
    // async calls
    let mut test = runtime.block_on(K8sEnv::new());
    let test_ns = runtime.block_on(test.test_namespace());
    // sync client initialization
    let k8s_client = Arc::new(SyncK8sClient::try_new(runtime, test_ns.clone()).unwrap());
    // ...
}
```
