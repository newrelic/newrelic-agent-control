use meta_agent::agent;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = agent::work() {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    Ok(())
}
