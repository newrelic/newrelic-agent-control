use meta_agent::{cli, supervisor::supervisor_group::SupervisorGroup};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ok, what would be the lifecycle of the meta agent?
    // I expect it to be something like:
    // There is a setup phase, in which the meta agent config is retrieved from the CLI or default,
    // then, for each agent in the config, a supervisor is created, and the agent is started.
    // The supervisor will be in charge of restarting the agent if it fails.
    // The meta agent will be in charge of stopping the supervisor if it receives a signal.
    // If there is a signal to finish the meta agent, the meta agent will stop all the supervisors

    // Initial setup phase
    let meta_agent_configs = cli::init_meta_agent()?;

    println!("Hello, world!");
    println!("config: {:?}", meta_agent_configs);
    println!(
        "I should be overseeing {} agents",
        meta_agent_configs.agents.len()
    );

    // Create the supervisor group
    let _supervisor_group = SupervisorGroup::from(&meta_agent_configs);

    println!("Hello, world!");
    println!("config: {:?}", meta_agent_configs);
    println!(
        "I should be overseeing {} agents",
        meta_agent_configs.agents.len()
    );

    Ok(())
}
