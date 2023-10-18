use std::fmt;
use std::str::FromStr;
use clap::builder::PossibleValue;
use clap::ValueEnum;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AgentRunningMode{
    Kubernetes,
    OnHost,
}

impl FromStr for AgentRunningMode {
    type Err = String;
    fn from_str(input: &str) -> Result<AgentRunningMode, Self::Err> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().expect("to_possible_value should cover all running modes").matches(input, false) {
                return Ok(*variant)
            }
        }
        Err(format!("invalid variant: {input}"))
    }
}
impl fmt::Display for AgentRunningMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.to_possible_value()
            .expect("to_possible_value should cover all running modes")
            .get_name()
            .fmt(f)
    }
}

impl clap::ValueEnum for AgentRunningMode{
    fn value_variants<'a>() -> &'a [AgentRunningMode] {
        &[AgentRunningMode::OnHost, AgentRunningMode::Kubernetes]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            AgentRunningMode::OnHost => PossibleValue::new("OnHost"),
            AgentRunningMode::Kubernetes =>PossibleValue::new("Kubernetes"),
        })
    }
}
