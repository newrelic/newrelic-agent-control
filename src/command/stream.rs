/// Stream of output events, either stdout or stderr
#[derive(Debug)]
pub enum OutputEvent {
    Stdout(String),
    Stderr(String),
}
