use crate::common::runtime::block_on;
use futures::{AsyncBufReadExt, StreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, WatchEvent};
use kube::api::{LogParams, WatchParams};
use kube::runtime::reflector::Lookup;
use std::time::Duration;
pub const AC_LABEL_SELECTOR: &str = "app.kubernetes.io/name=agent-control";
/// Watches for newly created pods matching the specified label selector and spawns a logger for each pod.
pub fn print_pod_logs(client: Client, namespace: &str, label_selector: &str) {
    let selector = label_selector.to_string();
    let ns = namespace.to_string();
    std::thread::spawn(move || {
        let pods: Api<Pod> = Api::namespaced(client.clone(), &ns);
        let wp = WatchParams::default().labels(&selector);
        let mut pods_stream = block_on(pods.watch(&wp, "0")).unwrap().boxed();
        while let Some(event_res) = block_on(pods_stream.next()) {
            if let Ok(WatchEvent::Added(pod)) = event_res {
                spawn_pod_logger(client.clone(), ns.clone(), pod.name().unwrap().to_string())
            }
        }
    });
}
/// Spawns a logger for the specified pod that continuously prints its logs to stdout.
pub fn spawn_pod_logger(client: Client, namespace: String, pod_name: String) {
    std::thread::spawn(move || {
        let pods: Api<Pod> = Api::namespaced(client, &namespace);
        let log_params = LogParams {
            follow: true,
            ..Default::default()
        };
        let mut lines_stream = loop {
            match block_on(pods.log_stream(&pod_name, &log_params)) {
                Ok(stream) => break stream.lines(),
                Err(err) => {
                    println!(
                        "Failed to get log stream for pod {}: {}. Retrying...",
                        pod_name, err
                    );
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        };
        while let Some(Ok(line)) = block_on(lines_stream.next()) {
            println!("{} {}", pod_name, line);
        }
    });
}
