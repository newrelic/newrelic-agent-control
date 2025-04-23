use super::logger::Logger;
use crate::{
    event::broadcaster::unbounded::UnboundedBroadcast, utils::threads::spawn_named_thread,
};

use std::{
    io::{BufRead, BufReader, Read},
    thread::JoinHandle,
};

pub(crate) fn spawn_logger<R>(handle: R, loggers: Vec<Logger>)
where
    R: Read + Send + 'static,
{
    if !loggers.is_empty() {
        // Forward to an inner function that returns the thread handles,
        // for ease of testing log outputs (we wait on them)
        spawn_logger_inner(handle, loggers);
    }
}

fn spawn_logger_inner<R>(handle: R, loggers: Vec<Logger>) -> (JoinHandle<()>, Vec<JoinHandle<()>>)
where
    R: Read + Send + 'static,
{
    let mut loggers_channel = UnboundedBroadcast::default();

    let log_threads = loggers
        .into_iter()
        .map(|logger| logger.log(loggers_channel.subscribe()))
        .collect();

    // In a separate thread, iterate over the handle to get the logs
    let sender_thread = spawn_named_thread("OnHost log sender", move || {
        let log_entries = BufReader::new(handle).lines();
        for line in log_entries {
            let line = line.expect("Failed to read line from buffered reader");
            // Send each line to all existing loggers.
            loggers_channel.broadcast(line);
        }
    });

    // Return the threads (for testing purposes)
    (sender_thread, log_threads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::sub_agent::on_host::command::logging::file_logger::FileLogger;
    use mockall::predicate::*;
    use mockall::{mock, Sequence};
    use std::io::{Read, Seek, SeekFrom, Write};
    use tempfile::tempfile;
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    mock! {
        Write {}

        impl Write for Write{
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize>;
            fn flush(&mut self) -> std::io::Result<()>;
        }
    }

    mock! {
        Read {}

        impl Read for Read {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
        }
    }

    #[test]
    fn spawn_empty_logger() {
        let mut read_mock = MockRead::new();
        // Current implementation should never actually read when passing an empty logger list
        read_mock.expect_read().never();

        let loggers = vec![];

        spawn_logger(read_mock, loggers);
    }

    #[traced_test]
    #[test]
    fn spawn_stdout_logger() {
        let log_lines = b"logging test 1\nlogging test 2\n";
        let mut read_mock = MockRead::new();

        // Reading in sequence
        let mut seq = Sequence::new();
        read_mock
            .expect_read()
            .once()
            .in_sequence(&mut seq)
            .returning(|mut buf| {
                buf.write_all(log_lines).unwrap();
                Ok(log_lines.len())
            });
        // No more contents to read
        read_mock
            .expect_read()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(0));

        let loggers = vec![Logger::Stdout(AgentID::new_agent_control_id())];

        let (sender_thd, logger_thds) = spawn_logger_inner(read_mock, loggers);
        sender_thd.join().unwrap();
        for thd in logger_thds {
            thd.join().unwrap();
        }

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
            "logging test 1",
        ));
        assert!(logs_with_scope_contain(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
            "logging test 2",
        ));
    }

    #[traced_test]
    #[test]
    fn spawn_stderr_logger() {
        let log_lines = b"err logging test 1\nerr logging test 2\n";
        let mut read_mock = MockRead::new();

        // Reading in sequence
        let mut seq = Sequence::new();
        read_mock
            .expect_read()
            .once()
            .in_sequence(&mut seq)
            .returning(|mut buf| {
                buf.write_all(log_lines).unwrap();
                Ok(log_lines.len())
            });
        // No more contents to read
        read_mock
            .expect_read()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(0));

        let loggers = vec![Logger::Stderr(AgentID::new_agent_control_id())];

        // I wait for the logging threads to finish and return to make assertions, otherwise
        // the test will assert before the threads are done and the logs are printed, failing.
        let (sender_thd, logger_thds) = spawn_logger_inner(read_mock, loggers);
        sender_thd.join().unwrap();
        for thd in logger_thds {
            thd.join().unwrap();
        }

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
            "err logging test 1",
        ));
        assert!(logs_with_scope_contain(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
            "err logging test 2",
        ));
    }

    #[traced_test]
    #[test]
    fn spawn_logger_with_file_logging() {
        // Create a writer and from it build a Logger::File(FileLogger)
        let agent_id = AgentID::new("test-agent").unwrap();
        let mut temp_file = tempfile().unwrap();
        let file_logger = Logger::File(
            FileLogger::from(temp_file.try_clone().unwrap()),
            agent_id.clone(),
        );

        let mut read_mock = MockRead::new();
        // Reading in sequence
        let mut seq = Sequence::new();
        read_mock
            .expect_read()
            .once()
            .in_sequence(&mut seq)
            .returning(|mut buf| {
                let log_lines = b"logging test 1\nlogging test 2\n";
                buf.write_all(log_lines).unwrap();
                Ok(log_lines.len())
            });
        // No more contents to read
        read_mock
            .expect_read()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(0));

        let loggers = vec![Logger::Stdout(agent_id), file_logger];

        let (sender_thd, logger_thds) = spawn_logger_inner(read_mock, loggers);
        sender_thd.join().unwrap();
        for thd in logger_thds {
            thd.join().unwrap();
        }

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
            "logging test 1 agent_id=test-agent",
        ));
        assert!(logs_with_scope_contain(
            "DEBUG newrelic_agent_control::sub_agent::on_host::command::logging::logger",
            "logging test 2 agent_id=test-agent",
        ));

        // Check the file content
        temp_file.seek(SeekFrom::Start(0)).unwrap();
        let mut content = String::new();
        temp_file.read_to_string(&mut content).unwrap();
        let expected =
            "logging test 1 agent_id=test-agent\nlogging test 2 agent_id=test-agent\n".to_string();
        assert_eq!(content, expected);
    }
}
