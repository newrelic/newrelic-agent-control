use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::agent_type::agent_types::VariableType;

use super::kind::Kind;

// Spec can be an arbitrary number of nested mappings but all node terminal leaves are EndSpec,
// so a recursive datatype is the answer!
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum Spec {
    SpecEnd(EndSpec),
    SpecMapping(HashMap<String, Spec>),
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct EndSpec {
    pub(crate) description: String,
    #[serde(flatten)]
    pub kind: Kind,
    // pub required: bool,
}

impl EndSpec {
  pub fn variable_type(&self) -> VariableType {
    self.kind.variable_type()
  }

  pub fn is_required(&self) -> bool {
    self.kind.is_required()
  }


}
