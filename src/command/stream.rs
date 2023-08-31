/// Stream of output events, either stdout or stderr
#[derive(Debug)]
pub enum OutputEvent {
    Stdout(String),
    Stderr(String),
}

// TODO/N2H: Switch to HashMap so it can use a list of key/values
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Metadata(String);

impl Metadata {
    pub fn new<V>(value: V) -> Self
    where
        V: ToString,
    {
        Metadata(value.to_string())
    }

    pub fn values(self) -> String {
        self.0
    }
}

#[derive(Debug)]
pub struct Event {
    pub output: OutputEvent,
    pub metadata: Metadata,
}
