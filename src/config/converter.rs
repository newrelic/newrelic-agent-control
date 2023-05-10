#[derive(Debug, Clone)]
pub(crate) struct ConvertError;

pub(crate) trait Convertible {
    /// Convert applies the conversion logic to the config from the type passed in the Generic C
    fn convert<C>(&self, config: C) -> Result<C, ConvertError>;
}
