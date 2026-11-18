//! Spawns shared by the backends that keep the child as a `std::process::Child`.
use crate::error::{Result, ctx};
use std::process::{Child, Command};

pub fn command(argv: &[String]) -> Result<Command> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| crate::error::Error::new("empty command"))?;
    let mut command = Command::new(program);
    command.args(args);
    Ok(command)
}

pub fn spawn(mut command: Command) -> Result<Child> {
    command.spawn().map_err(ctx("spawning process"))
}
