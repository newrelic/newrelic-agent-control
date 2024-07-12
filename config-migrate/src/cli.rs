use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    #[arg(short, long, default_value_t = String::from("/etc/newrelic-super-agent/config.yaml"))]
    config: String,
}

impl Cli {
    /// Parses command line arguments
    pub fn init_config_migrate_cli() -> Self {
        // Get command line args
        Self::parse()
    }

    pub fn get_config(&self) -> String {
        self.config.clone()
    }
}
