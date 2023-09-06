#[cfg(test)]
pub(crate) mod test {
    use mockall::mock;
    use opamp_client::{OpAMPClient, OpAMPClientHandle};
    use thiserror::Error;

    #[derive(Error, Debug)]
    #[error("callback error mock")]
    pub(crate) struct OpampClientMockError;

    #[derive(Error, Debug)]
    #[error("callback error mock")]
    pub(crate) struct OpampClientHandleMockError;

    mock! {
      pub(crate) OpampClientHandleMockall {}

      impl OpAMPClientHandle for OpampClientHandleMockall {
            type Error = OpampClientHandleMockError;

         async fn stop(self) -> Result<(), Self::Error>;
        async fn set_agent_description(
            &mut self,
            description: &AgentDescription,
        ) -> Result<(), Self::Error>;

        fn agent_description(&self) -> Result<AgentDescription, Self::Error>;
        async fn set_health(&mut self, health: &AgentHealth) -> Result<(), Self::Error>;
        async fn update_effective_config(&mut self) -> Result<(), Self::Error>;
      }
    }


    mock! {
      pub(crate) OpampClientMockall {}

      impl OpAMPClient for OpampClientMockall {
            type Error = OpampClientMockError;
            type Handle = MockOpampClientHandleMockall;
            async fn start(self) -> Result<Self::Handle, Self::Error>;
      }
    }

    // impl MockCallbacksMockall {
    //     // pub fn should_on_connect(&mut self) {
    //     //     self.expect_on_connect().once().return_const(());
    //     // }
    //     //
    //     // pub fn should_on_message(&mut self, _: MessageData) {
    //     //     self.expect_on_message()
    //     //         .once()
    //     //         // .with(eq(msg)) // TODO
    //     //         .return_const(());
    //     // }
    // }
}
