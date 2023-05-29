use std::fmt::Debug;

use config::{builder::DefaultState, Config as Config_rs, ConfigBuilder, File};
use serde::Deserialize;

use crate::config::config::Error::Error;
use crate::config::config::Getter;
use crate::config::config::Result;

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
    pub fn from_path(path: &str) -> Self {
        let static_builder =
            Config_rs::builder().add_source(File::with_name(path));

        Self { static_builder }
    }
}

/// The implementation of the config::Getter uses config_rs to deserialize the config loaded in the
/// config_builder into a config::Config
impl<C> Getter<C> for Resolver
where
        C: Debug + for<'a> Deserialize<'a>,
{
    fn get(&self) -> Result<C> {
        match self.static_builder.to_owned().build() {
            // TODO the error should be handled or panics when unwrapping
            Ok(config_rs) => Ok(config_rs.try_deserialize::<C>().unwrap()),
            Err(e) => {
                println!("{:?}", e);
                Err(Error(e.to_string()))
            }
        }
    }
}
