use crate::{tools::remove_dirs, windows::service};

const AGENT_CONTROL_DIRS: &[&str] = &[
    r"C:\Program Files\New Relic\newrelic-agent-control\",
    r"C:\ProgramData\New Relic\newrelic-agent-control\",
];

/// Tool to show logs when a test is over
pub struct CleanAcOnDrop<'a> {
    service_name: &'a str,
}

impl<'a> From<&'a str> for CleanAcOnDrop<'a> {
    fn from(value: &'a str) -> Self {
        Self {
            service_name: value,
        }
    }
}

impl<'a> Drop for CleanAcOnDrop<'a> {
    fn drop(&mut self) {
        service::stop_service(self.service_name);
        remove_dirs(AGENT_CONTROL_DIRS).expect("expected directories to be removed");
    }
}
