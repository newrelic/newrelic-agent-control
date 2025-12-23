use crate::{linux::bash::exec_bash_command, tools::test::TestResult};

/// Restarts a service using systemctl
pub fn restart_service(service_name: &str) -> TestResult<()> {
    let cmd = format!("systemctl restart {service_name}");
    let _ = exec_bash_command(&cmd, "restart service")?;
    Ok(())
}
