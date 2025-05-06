use std::str::FromStr;

use clap::Parser;
use kube::{api::DynamicObject, core::Duration};
use tracing::info;

use crate::{errors::ParseError, resources::HelmReleaseData, resources::HelmRepositoryData};

const REPOSITORY_NAME: &str = "newrelic";
const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";

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
        info!("Creating Agent Control resources dynamic object representations");

        let helm_repository = HelmRepositoryData {
            name: REPOSITORY_NAME.to_string(),
            url: REPOSITORY_URL.to_string(),
            labels: value.labels.clone(),
            annotations: value.annotations.clone(),
            interval: Duration::from_str("5m").expect("Hardcoded value should be correct"),
        };
        let repository_object = DynamicObject::try_from(helm_repository)?;

        let helm_release = HelmReleaseData {
            name: "agent-control-deployment-release".to_string(),
            chart_name: "agent-control-deployment".to_string(),
            chart_version: value.chart_version,
            repository_name: REPOSITORY_NAME.to_string(),
            values: value.values,
            labels: value.labels,
            annotations: value.annotations,
            interval: Duration::from_str("5m").expect("Hardcoded value should be correct"),
            timeout: Duration::from_str("5m").expect("Hardcoded value should be correct"),
        };
        let release_object = DynamicObject::try_from(helm_release)?;

        Ok(vec![repository_object, release_object])
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_to_dynamic_objects() {
        let data = AgentControlData {
            release_name: "agent-control-deployment-release".to_string(),
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
        assert_eq!(metadata.name, Some("agent-control-deployment-release".to_string()));
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
