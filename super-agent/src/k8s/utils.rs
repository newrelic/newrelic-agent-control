use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

pub enum IntOrPercentage {
    Int(i32),
    Percentage(f32),
    Unknown(std::num::ParseIntError),
}

impl From<IntOrString> for IntOrPercentage {
    fn from(value: IntOrString) -> Self {
        match value {
            IntOrString::Int(i) => IntOrPercentage::Int(i),
            IntOrString::String(s) => IntOrPercentage::from(s),
        }
    }
}

impl From<String> for IntOrPercentage {
    fn from(value: String) -> Self {
        match value.strip_suffix('%') {
            Some(percent) => match percent.parse::<i32>() {
                Ok(i) => IntOrPercentage::Percentage(i as f32 / 100.0),
                Err(err) => IntOrPercentage::Unknown(err),
            },
            None => match value.parse::<i32>() {
                Ok(i) => IntOrPercentage::Int(i),
                Err(err) => IntOrPercentage::Unknown(err),
            },
        }
    }
}

pub enum DaemonSetUpdateStrategies {
    RollingUpdate,
    OnDelete,
    Unknown(String),
}

const ROLLING_UPDATE: &str = "RollingUpdate";
const ON_DELETE: &str = "OnDelete";

impl From<Option<String>> for DaemonSetUpdateStrategies {
    fn from(value: Option<String>) -> Self {
        match value {
            Some(string) => self::DaemonSetUpdateStrategies::from(string),
            None => Self::Unknown("Option for this `UpdateStrategies` is None".to_string()),
        }
    }
}

impl From<String> for DaemonSetUpdateStrategies {
    fn from(value: String) -> Self {
        return match value.as_str() {
            ROLLING_UPDATE => return Self::RollingUpdate,
            ON_DELETE => return Self::OnDelete,
            unknown => Self::Unknown(unknown.to_string()),
        };
    }
}

impl std::fmt::Display for DaemonSetUpdateStrategies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonSetUpdateStrategies::RollingUpdate => write!(f, "{ROLLING_UPDATE}"),
            DaemonSetUpdateStrategies::OnDelete => write!(f, "{ON_DELETE}"),
            DaemonSetUpdateStrategies::Unknown(s) => write!(f, "{s}"),
        }
    }
}

// This is a copy of `is_label_present` from PR #633 so we can reuse it. This is subject to change while that PR
// Is still on review.
pub fn is_label_present(labels: &Option<BTreeMap<String, String>>, key: &str, value: &str) -> bool {
    if let Some(labels) = labels.as_ref() {
        if let Some(v) = labels.get(key) {
            if v.as_str() == value {
                return true;
            }
        }
    }
    false
}
