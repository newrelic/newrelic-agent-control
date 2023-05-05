pub trait Convertable {
    // Convert applies the conversion logic to the given "conf".
    //async fn convert(&self, conf: &Conf) -> Result<(), Box<dyn Error>>;
    fn len(&self) -> usize;
}
