use std::thread;

use log::info;

use crate::{
    agent::{lifecycle::Lifecycle, signal::SignalManager, supervisor_group::SupervisorGroup},
    command::{EventLogger, StdEventReceiver},
};

mod error;
pub mod lifecycle;
pub mod logging;
pub mod signal;
pub mod supervisor_group;

pub struct Agent;

impl Agent {
    pub fn work() -> Result<(), Box<dyn std::error::Error>> {
        // Initial setup phase
        let mut init = Lifecycle::init()?;

        // FIXME: Placeholder for NR-124576
        info!("Starting the signal manager");
        let signal_manager = SignalManager::new(init.get_context()).shutdown_handle();

        let (tx, rx) = init.extract_channel()?;

        let output_manager = StdEventReceiver::default().log(rx);

        let supervisor_group = SupervisorGroup::new(init.get_context(), tx, init.get_configs());
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
}
