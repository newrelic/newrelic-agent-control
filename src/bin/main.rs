use meta_agent::cli;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _config = cli::init_meta_agent()?;

    println!("Hello, world!");
    println!("config: {:?}", _config);
    println!("I should be overseeing {} agents", _config.agents.len());

    Ok(())
}
