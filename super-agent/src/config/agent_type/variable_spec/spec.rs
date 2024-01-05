use std::collections::HashMap;

use serde::Deserialize;

use super::kind::Kind;

// Spec can be an arbitrary number of nested mappings but all node terminal leaves are EndSpec,
// so a recursive datatype is the answer!
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum Spec {
    SpecEnd(EndSpec),
    SpecMapping(HashMap<String, Spec>),
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct EndSpec {
    pub(crate) description: String,
    #[serde(flatten)]
    pub kind: Kind,
    // pub required: bool,
}
