use std::{fs, path::PathBuf, sync::Arc};

use clap::Parser;
use kube::api::{DynamicObject, ObjectMeta};
use newrelic_agent_control::{
    agent_control::config::helmrelease_v2_type_meta, k8s::client::SyncK8sClient,
};
use tracing::info;

use crate::utils::parse_key_value_pairs;

#[derive(Parser)]
pub struct HelmReleaseData {
    /// Object name
    #[arg(long)]
    pub name: String,

    /// Name of the chart to deploy
    #[arg(long)]
    pub chart_name: String,

    /// Version of the chart to deploy
    #[arg(long)]
    pub chart_version: String,

    /// Name of the Helm Repository from where to get the chart
    ///
    /// The Helm Repository must already be created in the
    /// cluster.
    #[arg(long)]
    pub repository_name: String,

    /// Chart values as string
    ///
    /// The values of the chart as a yaml string.
    /// The values can also be read from a file using `--values-file`,
    /// but only one flag can be used at once.
    #[arg(long)]
    pub values: Option<String>,

    /// Chart values file
    ///
    /// A yaml file with the values of the chart.
    /// The values can also be passed as a string with `--values`,
    /// but only one flag can be used at once.
    #[arg(long)]
    pub values_file: Option<PathBuf>,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    #[arg(long)]
    pub labels: Option<String>,

    /// Non-identifying metadata
    #[arg(long)]
    pub annotations: Option<String>,

    /// Interval at which the release is reconciled
    ///
    /// The controller will check the Helm release is in
    /// the desired state at the specified interval.
    ///
    /// The interval must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    pub interval: String,

    /// Timeout for some Helm actions
    ///
    /// Some Helm actions like install, upgrade or rollback
    /// will timeout at the specified elapsed time.
    ///
    /// The timeout must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    pub timeout: String,
}

pub fn create_helm_release(k8s_client: Arc<SyncK8sClient>, helm_release_data: HelmReleaseData) {
    info!("Creating Helm release");

    let mut data = serde_json::json!({
        "spec": {
            "interval": helm_release_data.interval,
            "timeout": helm_release_data.timeout,
            "chart": {
                "spec": {
                    "chart": helm_release_data.chart_name,
                    "version": helm_release_data.chart_version,
                    "sourceRef": {
                        "kind": "HelmRepository",
                        "name": helm_release_data.repository_name,
                    },
                    "interval": helm_release_data.interval,
                },
            }
        }
    });

    if let Some(values) = parse_helm_release_values(&helm_release_data) {
        data["spec"]["values"] = values;
    }

    let helm_release = DynamicObject {
        types: Some(helmrelease_v2_type_meta()),
        metadata: ObjectMeta {
            name: Some(helm_release_data.name.clone()),
            namespace: Some(k8s_client.default_namespace().to_string()),
            annotations: parse_key_value_pairs(&helm_release_data.annotations.unwrap_or_default()),
            labels: parse_key_value_pairs(&helm_release_data.labels.unwrap_or_default()),
            ..Default::default()
        },
        data,
    };
    info!("Helm release object created: {:?}", helm_release);

    info!("Applying helm release");
    k8s_client.apply_dynamic_object(&helm_release).unwrap();
    info!("Helm release applied.");
}

pub fn parse_helm_release_values(helm_release_data: &HelmReleaseData) -> Option<serde_json::Value> {
    match (&helm_release_data.values, &helm_release_data.values_file) {
        (Some(_), Some(_)) => {
            panic!("You can only specify one of --values or --values-file");
        }
        (Some(values), None) => {
            let values = serde_yaml::from_str(&values).unwrap();
            Some(serde_json::from_value(values).unwrap())
        }
        (None, Some(values_file)) => {
            let values = fs::read_to_string(values_file).unwrap();
            let values = serde_yaml::from_str(&values).unwrap();
            Some(serde_json::from_value(values).unwrap())
        }
        (None, None) => None,
    }
}
