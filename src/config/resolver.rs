use config::{builder::DefaultState, Config as Config_rs, ConfigBuilder, File, FileFormat};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;

use crate::agent::config::{Config, Getter};

const DEFAULT_STATIC_CONFIG: &str = "/tmp/static.yaml";

/// The Resolver contains an static_builder to build config parser, the crate config_rs is used
/// that allows registering ordered sources of configuration in multiple supported file formats
/// to later build consistent configs
#[derive(Debug)]
pub struct Resolver {
    static_builder: ConfigBuilder<DefaultState>,
}

/// The Resolver implementation defines a constructor that will define a single config file source,
/// that is defined from the default static config.
impl Resolver {
    pub fn new() -> Self {
        let static_builder =
            Config_rs::builder().add_source(File::new(DEFAULT_STATIC_CONFIG, FileFormat::Yaml));

        Self { static_builder }
    }
}

/// The implementation of the config::Getter uses config_rs to deserialize the config loaded in the
/// config_builder into a config::Config
impl<V> Getter<V> for Resolver
where
    V: Debug + for<'a> Deserialize<'a>,
{
    fn get(&self) -> Config<V> {
        match self.static_builder.to_owned().build() {
            // TODO the error should be handled or panics when unwrapping
            Ok(config_rs) => config_rs.try_deserialize::<Config<V>>().unwrap(),
            Err(e) => {
                println!("{:?}", e);
                Config {
                    op_amp: "".to_string(),
                    agents: HashMap::new(),
                }
            }
        }
    }
}
