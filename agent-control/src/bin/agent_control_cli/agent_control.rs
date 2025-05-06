use std::{collections::BTreeMap, str::FromStr};

use clap::Parser;
use k8s_openapi::api::core::v1::Secret;
use kube::{
    api::{DynamicObject, ObjectMeta},
    core::Duration,
};
use tracing::{debug, info};

use crate::{
    errors::ParseError,
    resources::{HelmReleaseData, HelmRepositoryData, SecretData},
    utils::parse_key_value_pairs,
};

const REPOSITORY_NAME: &str = "newrelic";
const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";
const SECRET_NAME: &str = "agent-control-secret";

#[derive(Debug, Parser)]
pub struct AgentControlData {
    /// Release name
    #[arg(long)]
    pub release_name: String,

    /// Version of the agent control chart
    #[arg(long)]
    pub chart_version: String,

    /// Chart values
    ///
    /// A yaml file or yaml string with the values of the chart.
    /// If the value starts with `fs://`, it is treated as a
    /// file path. Otherwise, it is treated as a string.
    #[arg(long)]
    pub values: Option<String>,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    /// They will be applied to every resource created for Agent Control.
    #[arg(long)]
    pub labels: Option<String>,

    /// Non-identifying metadata
    ///
    /// They will be applied to every resource created for Agent Control.
    #[arg(long)]
    pub annotations: Option<String>,
}

impl TryFrom<AgentControlData> for Vec<DynamicObject> {
    type Error = ParseError;

    fn try_from(value: AgentControlData) -> Result<Self, Self::Error> {
        info!("Creating Agent Control resources representations");

        let labels = parse_key_value_pairs(value.labels.as_deref().unwrap_or_default());
        debug!("Parsed labels: {:?}", labels);

        let annotations = parse_key_value_pairs(value.annotations.as_deref().unwrap_or_default());
        debug!("Parsed annotations: {:?}", annotations);

        let helm_repository = HelmRepositoryData {
            name: REPOSITORY_NAME.to_string(),
            url: REPOSITORY_URL.to_string(),
            labels: labels.clone(),
            annotations: annotations.clone(),
            interval: Duration::from_str("5m").expect("Hardcoded value should be correct"),
        };
        let repository_object = DynamicObject::try_from(helm_repository)?;

        let values = value.values.map(parse_values).transpose()?;
        let string_data = values.map(|v| BTreeMap::from_iter(vec![("values.yaml".to_string(), v)]));
        let secret = SecretData(Secret {
            type_: Some("Opaque".to_string()),
            metadata: ObjectMeta {
                name: Some(SECRET_NAME.to_string()),
                labels: labels.clone(),
                annotations: annotations.clone(),
                ..Default::default()
            },
            string_data,
            data: None,
            immutable: None,
        });
        let secret_object = DynamicObject::try_from(secret)?;

        let helm_release = HelmReleaseData {
            name: value.release_name,
            chart_name: "agent-control-deployment".to_string(),
            chart_version: value.chart_version,
            repository_name: REPOSITORY_NAME.to_string(),
            values: None,
            values_from_secret: Some(SECRET_NAME.to_string()),
            labels,
            annotations,
            interval: Duration::from_str("5m").expect("Hardcoded value should be correct"),
            timeout: Duration::from_str("5m").expect("Hardcoded value should be correct"),
        };
        let release_object = DynamicObject::try_from(helm_release)?;

        info!("Agent Control resources representations created");

        Ok(vec![repository_object, secret_object, release_object])
    }
}

fn parse_values(values: String) -> Result<String, ParseError> {
    let values = match values.strip_prefix("fs://") {
        Some(path) => std::fs::read_to_string(path)?,
        None => values,
    };

    let yaml_values = serde_yaml::from_str::<serde_yaml::Value>(&values)?;
    let values = serde_json::to_string(&yaml_values).expect("YAML to JSON should work if input string was valid");

    Ok(values)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_to_dynamic_objects() {
        let release_name = "agent-control-deployment-release".to_string();
        let data = AgentControlData {
            release_name: release_name.clone(),
            chart_version: "1.0.0".to_string(),
            values: None,
            labels: Some("key1=value1,key2=value2".to_string()),
            annotations: Some("annotation1=value1,annotation2=value2".to_string()),
        };

        let dynamic_objects = Vec::<DynamicObject>::try_from(data).unwrap();

        assert_eq!(dynamic_objects.len(), 2);

        // Check the repository object
        let data = &dynamic_objects[0].data;
        assert_eq!(data["spec"]["url"], REPOSITORY_URL);
        assert_eq!(data["spec"]["interval"], "300s");

        let metadata = &dynamic_objects[0].metadata;
        assert_eq!(metadata.name, Some(REPOSITORY_NAME.to_string()));
        assert_eq!(
            metadata.labels,
            Some(BTreeMap::from_iter(vec![
                ("key1".to_string(), "value1".to_string()),
                ("key2".to_string(), "value2".to_string()),
            ]))
        );
        assert_eq!(
            metadata.annotations,
            Some(BTreeMap::from_iter(vec![
                ("annotation1".to_string(), "value1".to_string()),
                ("annotation2".to_string(), "value2".to_string()),
            ]))
        );

        // Check the release object
        let data = &dynamic_objects[1].data;
        assert_eq!(
            data["spec"]["chart"]["spec"]["sourceRef"]["name"],
            REPOSITORY_NAME
        );
        assert_eq!(data["spec"]["interval"], "300s");
        assert_eq!(data["spec"]["timeout"], "300s");

        let metadata = &dynamic_objects[1].metadata;
        assert_eq!(metadata.name, Some(release_name));
        assert_eq!(
            metadata.labels,
            Some(BTreeMap::from_iter(vec![
                ("key1".to_string(), "value1".to_string()),
                ("key2".to_string(), "value2".to_string()),
            ]))
        );
        assert_eq!(
            metadata.annotations,
            Some(BTreeMap::from_iter(vec![
                ("annotation1".to_string(), "value1".to_string()),
                ("annotation2".to_string(), "value2".to_string()),
            ]))
        );
    }
}
