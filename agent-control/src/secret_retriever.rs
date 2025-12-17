pub mod k8s;
pub mod on_host;

pub trait OpampSecretRetriever {
    type Error: std::error::Error;
    fn retrieve(&self) -> Result<String, Self::Error>;
}
