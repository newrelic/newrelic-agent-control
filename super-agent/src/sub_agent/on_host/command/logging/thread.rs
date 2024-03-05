use std::{
    io::{BufRead, BufReader, Read},
    sync::mpsc::{SendError, Sender},
};

use tracing::{error, info};

use crate::sub_agent::{
    logger::{AgentLog, Metadata},
    on_host::command::command::CommandError,
};

use super::{
    file_logger::FileLogger,
    handle::{LogHandle, ToLogOutput},
};

pub(crate) fn spawn_logger(
    fields: Metadata,
    sender: Sender<AgentLog>,
    handle: LogHandle,
    file_logger: Option<FileLogger>,
) {
    std::thread::spawn({
        move || {
            // Modifying the logger to use the file_logger for this thread, if provided
            let guard = file_logger.map(FileLogger::set_file_logging);

            let log_output_fn: ToLogOutput = (&handle).into();
            let processed_events = process_events(handle, |line| {
                // Our custom formatter will look for the `file_log_line` field.
                if guard.is_some() {
                    info!(file_log_line = line); // captured by file_subscriber
                }
                sender.send(AgentLog {
                    metadata: fields.clone(),
                    output: log_output_fn(line),
                })
            });
            // Resetting logger to default for this thread, if provided
            drop(guard);

            processed_events.map_err(|e| error!("stream error: {}", e))
        }
    });
}

fn process_events<R, F>(stream: R, send: F) -> Result<(), CommandError>
where
    R: Read,
    F: Fn(String) -> Result<(), SendError<AgentLog>>,
{
    let out = BufReader::new(stream).lines();
    for line in out {
        send(line?)?;
    }
    Ok(())
}
