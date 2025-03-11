use crate::utils::thread_context::ThreadContextStopperError;

pub trait SupervisorStopper {
    fn stop(self) -> Result<(), ThreadContextStopperError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::sub_agent::supervisor::stopper::SupervisorStopper;
    use crate::utils::thread_context::ThreadContextStopperError;
    use mockall::mock;

    mock! {
        pub SupervisorStopper {}

        impl SupervisorStopper for SupervisorStopper{
        fn stop(self) -> Result<(), ThreadContextStopperError>;
        }
    }

    impl MockSupervisorStopper {
        pub fn should_stop(&mut self) {
            self.expect_stop().once().return_once(|| Ok(()));
        }
    }
}
