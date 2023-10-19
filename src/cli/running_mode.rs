use clap::builder::PossibleValue;
use clap::ValueEnum;
use std::fmt;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AgentRunningMode {
    Kubernetes,
    OnHost,
}

impl fmt::Display for AgentRunningMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.to_possible_value()
            .expect("to_possible_value should cover all running modes")
            .get_name()
            .fmt(f)
    }
}

impl clap::ValueEnum for AgentRunningMode {
    fn value_variants<'a>() -> &'a [AgentRunningMode] {
        &[AgentRunningMode::OnHost, AgentRunningMode::Kubernetes]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            AgentRunningMode::OnHost => PossibleValue::new("OnHost"),
            AgentRunningMode::Kubernetes => PossibleValue::new("Kubernetes"),
        })
    }
}
