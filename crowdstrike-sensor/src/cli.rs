use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    #[arg(long)]
    #[clap(required = true)]
    pub client_id: String,
    #[arg(long)]
    #[clap(required = true)]
    pub client_secret: String,
}

impl Cli {
    /// Parses command line arguments
    pub fn init() -> Self {
        // Get command line args
        Self::parse()
    }
}
