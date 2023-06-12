use std::thread;

use log::info;
use meta_agent::{
    agent::{logging, supervisor_group::SupervisorGroup},
    cli,
    command::{EventLogger, StdEventReceiver},
    supervisor::context::SupervisorContext,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initial setup phase
    info!("Starting the meta agent");
    let meta_agent_configs = cli::init_meta_agent()?;

    // Start logger (will be influenced by the meta agent config, and implementation should be hidden behind a trait)
    logging::init()?;

    info!("Creating communication channels");
    let (tx, rx) = std::sync::mpsc::channel();

    info!("Creating the supervisor context");
    let ctx = SupervisorContext::new();

    // FIXME: Placeholder for NR-124576
    let signal_manager = thread::spawn({
        let ctx = ctx.clone();
        move || {
            info!("Starting the signal manager");
            thread::sleep(std::time::Duration::from_secs(120));
            ctx.cancel_all().unwrap();
        }
    });

    let output_manager = StdEventReceiver::default().log(rx);

    let supervisor_group = SupervisorGroup::new(ctx, tx, &meta_agent_configs);
    {
        /*
            TODO: We should first compare the current config with the one in the meta agent config.
            In a future situation, it might have changed due to updates from OpAMP, etc.
            Then, this would require selecting the agents whose config has changed,
            and restarting them.

            FIXME: Given the above comment, this should be converted to a loop in which we modify
            the supervisor group behavior on config changes and selectively restart them as needed.
            For checking the supervisors in a non-blocking way, we can use Handle::is_finished().

            Suppose there's a config change. Situations:
            - Current agents stay as is, new agents are added: start these new agents, merge them with the current group.
            - Current agents stay as is, some agents are removed: get list of these agents (by key), stop and remove them from the current group.
            - Updated config for a certain agent(s) (type, name). Get (by key), stop, remove from the current group, start again with the new config and merge with the running group.

            The "merge" operation can only be done if the agents are of the same type! Supervisor<Running>. If they are not started we won't be able to merge them to the running group, as they are different types.
        */

        info!("Starting the supervisor group.");
        // Run all the agents in the supervisor group
        let running_supervisors = supervisor_group.run();

        // For the time being, we only need to wait for the supervisor group to finish
        let _ = running_supervisors.wait();
        info!("Supervisor group has finished. Exiting the meta agent");
    }

    // Ending the program
    info!("Waiting for the signal manager to finish");
    signal_manager.join().unwrap();
    info!("Waiting for the output manager to finish");
    output_manager.join().unwrap();

    info!("Exit");

    Ok(())
}
