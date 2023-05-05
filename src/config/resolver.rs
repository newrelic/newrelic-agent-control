use std::collections::HashMap;
use config::{Config as Config_rs, ConfigBuilder, builder::DefaultState, File, FileFormat};
use std::fmt::Debug;
use serde::Deserialize;

use crate::agent::config::{Config, Getter};

const DEFAULT_STATIC_CONFIG: &str = "/tmp/static.yaml";

#[derive(Debug)]
pub struct Resolver {
    static_builder: ConfigBuilder<DefaultState>,
}

impl Resolver {
    pub fn new() -> Self {
        let static_builder = Config_rs::builder().
            add_source(File::new(DEFAULT_STATIC_CONFIG, FileFormat::Yaml));

        Self {
            static_builder,
        }
    }
}

impl<V> Getter<V> for Resolver where V: Debug + for<'a> Deserialize<'a> {
    fn get(&self) -> Config<V> {
        match self.static_builder.to_owned().build() {
            Ok(config_rs) => {
                config_rs
                    .try_deserialize::<Config<V>>()
                    .unwrap()
            },
            Err(e) => {
                println!("{:?}", e);
                Config{op_amp: "".to_string(), agents: HashMap::new()}
            }
        }
    }
}
