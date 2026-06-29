//! Agent types: their definition, parsing, registries, variables and the templating that turns a
//! parsed agent type plus user values into a rendered runtime configuration for a sub-agent.
pub mod agent_attributes;
pub mod agent_type_id;
pub mod definition;
pub mod error;
pub mod guid_config;
pub mod oci;
pub mod protocol_version;
pub mod registry;
pub mod render;
pub mod runtime_config;
pub mod templates;
pub mod templates_function;
pub mod trivial_value;
pub mod variable;
pub mod version_config;
