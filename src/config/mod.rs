pub(crate) mod agent_configs;
pub(crate) mod agent_type;
pub(crate) mod error;
pub(crate) mod resolver;

pub trait ConfigResolver {
    type Output;
    type Error;

    fn resolve(self) -> Result<Self::Output, Self::Error>;
}
