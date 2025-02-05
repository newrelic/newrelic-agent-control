use crate::sub_agent::thread_context::ThreadContextStopperError;

pub trait SupervisorStopper {
    fn stop(self) -> Result<(), ThreadContextStopperError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::sub_agent::supervisor::stopper::SupervisorStopper;
    use crate::sub_agent::thread_context::ThreadContextStopperError;
    use mockall::mock;

    mock! {
        pub SupervisorStopper {}

        impl SupervisorStopper for SupervisorStopper{
        fn stop(self) -> Result<(), ThreadContextStopperError>;
        }
    }
}
