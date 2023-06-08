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

    // TODO: List of things needed
    /*
    - [ ] When the config is initialized, should be wrapped in a RwLock. This will allow for future hot reloading of the config (OpAMP input will be the writer?).
    - [ ] Create a tx/rx pair to communicate with the supervisor group and log the outputs
    - [ ] Create the overall context
    - [ ] Spawn a thread to manage the received signals and quit the application (for the moment, this can just wait for like 2 minutes and then call ctx.cancel_all()). Placeholder for NR-124576
    - [ ] Spawn another thread to receive the outputs. Placeholder for NR-121865.
    - [ ] Spawn another thread for the supervisor group. Then, in a loop:
        - [ ] From the config (N2H: compare "current loop config" with fixed config, for hot reloading?), create the supervisor group.
        - [ ] For each supervisor in the group, start it, getting the handles. N2H: The contexts should be accessible for this loop (for hot reloading, finishing supervisors on demand depending on the config comparison done in the previous step).
        - [ ] In a branch, filter for the finished supervisors (use JoinHandle::is_finished(), non-blocking). This means that the agents have finished via exceeded retries for their max retries policy or some other reason.
        - [ ] Join the finished supervisors (should not block the loop for too much).
        - [ ] If the Result from the join() is an Err, log it as error!. Otherwise, log it as debug!.
        - [ ] Filter the original supervisor group to remove the finished ones
        - [ ] If there are no more supervisors, break the loop
        - [ ] Sleep for a while (1 second?)
        - [ ] Restart the loop

    - [ ] In another loop...
        - [ ] Wait for the three main threads to be finished (the signal manager, the output manager and the supervisor group manager)
            - [ ] If the signal manager is finished, the supervisor group should be finished as well soon and with it the output manager, so wait for them to finish.
            - [ ] If the supervisor group is finished, the output manager should be finished as well soon, so wait for it to finish. Forcefully finish the signal manager. (How? A context + condvar for the signal manager?)
            - [ ] If the output manager is finished but the other two remain, we have somehow lost communication with the supervisor group (but will this happen?). Forcefully finish the signal manager and the supervisor group. (How? I don't have the context at this point?)
     */

    Ok(())
}
