use crate::common::remove_dirs;
use crate::windows::service;
use tracing::error;

const AGENT_CONTROL_DIRS: &[&str] = &[
    r"C:\Program Files\New Relic\newrelic-agent-control\",
    r"C:\ProgramData\New Relic\newrelic-agent-control\",
];

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
        _ = remove_dirs(AGENT_CONTROL_DIRS).inspect_err(|err| {
            error!("Failed to remove Agent Control directories: {}", err);
        });
    }
}
