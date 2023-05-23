#[derive(Debug)]
pub enum OutputEvent {
    Stdout(String),
    Stderr(String),
}
