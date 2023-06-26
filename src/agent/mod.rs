use std::{
    path::Path,
    sync::mpsc::{self, Sender},
};

use tracing::{error, info};

use crate::{
    agent::supervisor_group::SupervisorGroup,
    command::{
        processrunner::ProcessRunnerBuilder, shutdown::ProcessTerminatorBuilder, stream::Event,
        CommandBuilder, EventLogger, StdEventReceiver, TerminatorBuilder,
    },
    config::{agent_configs::MetaAgentConfig, agent_type::AgentType, resolver::Resolver},
    context::Context,
    supervisor::runner::Stopped,
};

use self::error::AgentError;

pub mod error;
pub mod supervisor_group;

#[derive(Clone)]
pub enum AgentEvent {
    // this should be a list of agentTypes
    Restart(AgentType),
    // stop all supervisors
    Stop,
}

pub trait SupervisorGroupResolver {
    type ProcessBuilder: CommandBuilder + 'static;
    type TerminatorBuilder: TerminatorBuilder + 'static;
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
    ) -> SupervisorGroup<Stopped<Self::ProcessBuilder, Self::TerminatorBuilder>>;
}

impl SupervisorGroupResolver for MetaAgentConfig {
    type ProcessBuilder = ProcessRunnerBuilder;
    type TerminatorBuilder = ProcessTerminatorBuilder;
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
    ) -> SupervisorGroup<Stopped<ProcessRunnerBuilder, ProcessTerminatorBuilder>> {
        SupervisorGroup::new(tx, self)
    }
}

pub struct Agent<R = MetaAgentConfig>
where
    R: SupervisorGroupResolver,
{
    resolver: R,
}

impl Agent {
    pub fn new(cfg_path: &Path) -> Result<Self, AgentError> {
        let cfg = Resolver::retrieve_config(cfg_path)?;

        Ok(Self { resolver: cfg })
    }

    #[cfg(test)]
    fn new_custom_resolver<R>(resolver: R) -> Agent<R>
    where
        R: SupervisorGroupResolver,
    {
        Agent::<R> { resolver }
    }
}

impl<R> Agent<R>
where
    R: SupervisorGroupResolver,
{
    pub fn run(self, ctx: Context<Option<AgentEvent>>) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        let supervisor_group = self.resolver.retrieve_group(tx);
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

        // watch for supervisors restart requests
        {
            loop {
                // blocking wait until context is woken up
                if let Some(event) = ctx.wait_condvar().unwrap() {
                    match event {
                        AgentEvent::Stop => {
                            break running_supervisors.stop().into_iter().for_each(
                                |(supervisor, handle)| {
                                    handle.join().map_or_else(
                                        |_err| {
                                            // let error: &dyn std::error::Error = &err;
                                            error!(
                                                supervisor = String::from(&supervisor),
                                                msg = "stopped with error",
                                            )
                                        },
                                        |_| {
                                            info!(
                                                supervisor = String::from(&supervisor),
                                                msg = "stopped successfully"
                                            )
                                        },
                                    )
                                },
                            );
                        }

                        AgentEvent::Restart(_agent_type) => {
                            // restart the corresponding supervisor
                            // TODO: remove agent from map, stop, run and reinsert it again
                        }
                    };
                }
                // spurious condvar wake up, loop should continue
            }
        }

        info!("Waiting for the output manager to finish");
        output_manager.join().unwrap();

        info!("MetaAgent finished");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        thread::{sleep, spawn},
        time::Duration,
    };

    use crate::{
        command::{
            processrunner::sleep_process_builder::MockedProcessBuilder,
            shutdown::terminator_builder::NopTerminatorBuiler,
        },
        context::Context,
    };

    use super::{
        supervisor_group::tests::new_sleep_supervisor_group, Agent, AgentEvent,
        SupervisorGroupResolver,
    };

    struct MockedSleepGroupResolver;
    impl SupervisorGroupResolver for MockedSleepGroupResolver {
        type ProcessBuilder = MockedProcessBuilder;
        type TerminatorBuilder = NopTerminatorBuiler;
        fn retrieve_group(
            &self,
            tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
        ) -> super::supervisor_group::SupervisorGroup<
            crate::supervisor::runner::Stopped<MockedProcessBuilder, NopTerminatorBuiler>,
        > {
            new_sleep_supervisor_group(tx)
        }
    }

    #[test]
    fn run_and_stop_supervisors() {
        let agent = Agent::new_custom_resolver(MockedSleepGroupResolver);
        let ctx: Context<Option<AgentEvent>> = Context::new();

        // stop all agents after 3 seconds
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(Duration::from_secs(3));
                ctx.cancel_all(Some(AgentEvent::Stop)).unwrap();
            }
        });

        assert!(agent.run(ctx).is_ok())
    }
}
