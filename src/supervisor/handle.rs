use crate::command::{processrunner::Started, ProcessRunner};

use super::Handle;
pub(crate) struct SupervisorHandle {
    pub(crate) p_handle: ProcessRunner<Started>,
}
impl Handle for SupervisorHandle {
    type E = std::io::Error;

    fn stop(self) -> Result</* Self::R */ (), Self::E> {
        Ok(())
    }
}
