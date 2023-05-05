#[derive(Debug, Clone)]
struct ConvertError;

pub trait Convertible {
    // Convert applies the conversion logic to the given config.
    fn convert<C>(&self, config: C) -> Result<C, ConvertError>;
}
