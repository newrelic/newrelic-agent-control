//! Spawns reader/forwarder threads that fan out an executable's output lines to its loggers.

use super::logger::Logger;
use crate::{
    event::broadcaster::unbounded::UnboundedBroadcast, utils::threads::spawn_named_thread,
};

use std::{
    io::{BufRead, BufReader, Read},
    thread::JoinHandle,
};
use tracing::{dispatcher, error};

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

    let dispatch = dispatcher::get_default(|d| d.clone());

    // In a separate thread, iterate over the handle to get the logs
    let sender_thread = spawn_named_thread("OnHost log sender", move || {
        let _guard = dispatcher::set_default(&dispatch);

        // Split on raw bytes so lines with non-UTF-8 content (e.g. a Latin-1 file path) are kept.
        let log_entries = BufReader::new(handle).split(b'\n');
        for line in log_entries {
            let line = match line {
                Ok(line) => line,
                Err(err) => {
                    // Read errors on the pipe repeat on every call, retrying would loop forever.
                    error!("Failed to read executable output, stopping log forwarding: {err}");
                    break;
                }
            };
            let line = line.strip_suffix(b"\r").unwrap_or(&line);
            loggers_channel.broadcast(String::from_utf8_lossy(line).into_owned());
        }
    });

    // Return the threads (for testing purposes)
    (sender_thread, log_threads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::agent_id::tests::UniqueAgentID;
    use crate::sub_agent::on_host::command::logging::file_logger::FileLogger;
    use crate::sub_agent::on_host::test_utils::global_logs_lines;
    use mockall::predicate::*;
    use mockall::{Sequence, mock};
    use rstest::rstest;
    use std::io::{Read, Seek, SeekFrom, Write};
    use tempfile::tempfile;
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

        let agent_id = UniqueAgentID::build();
        let loggers = vec![Logger::Stdout(agent_id.clone().into())];

        let (sender_thd, logger_thds) = spawn_logger_inner(read_mock, loggers);
        sender_thd.join().unwrap();
        for thd in logger_thds {
            thd.join().unwrap();
        }

        let logs = global_logs_lines(agent_id);
        assert!(logs.iter().any(|l| l.contains("logging test 1")));
        assert!(logs.iter().any(|l| l.contains("logging test 2")));
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

        let agent_id = UniqueAgentID::build();
        let loggers = vec![Logger::Stderr(agent_id.clone().into())];

        // I wait for the logging threads to finish and return to make assertions, otherwise
        // the test will assert before the threads are done and the logs are printed, failing.
        let (sender_thd, logger_thds) = spawn_logger_inner(read_mock, loggers);
        sender_thd.join().unwrap();
        for thd in logger_thds {
            thd.join().unwrap();
        }

        let logs = global_logs_lines(agent_id);
        assert!(logs.iter().any(|l| l.contains("err logging test 1")));
        assert!(logs.iter().any(|l| l.contains("err logging test 2")));
    }

    #[traced_test]
    #[test]
    fn spawn_logger_with_file_logging() {
        // Create a writer and from it build a Logger::File(FileLogger)
        let agent_id = UniqueAgentID::build();
        let mut temp_file = tempfile().unwrap();
        let file_logger = Logger::File(Box::new(FileLogger::new(temp_file.try_clone().unwrap())));

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

        let loggers = vec![Logger::Stdout(agent_id.clone().into()), file_logger];

        let (sender_thd, logger_thds) = spawn_logger_inner(read_mock, loggers);
        sender_thd.join().unwrap();
        for thd in logger_thds {
            thd.join().unwrap();
        }

        let logs = global_logs_lines(agent_id.clone());
        let agent_id: AgentID = agent_id.into();
        assert!(
            logs.iter()
                .any(|l| l.contains(&format!("logging test 1 agent_id={agent_id}")))
        );
        assert!(
            logs.iter()
                .any(|l| l.contains(&format!("logging test 2 agent_id={agent_id}")))
        );

        // Check the file content: no agent_id decoration, the file already lives under a
        // per-agent directory.
        temp_file.seek(SeekFrom::Start(0)).unwrap();
        let mut content = String::new();
        temp_file.read_to_string(&mut content).unwrap();
        assert_eq!(content, "logging test 1\nlogging test 2\n");
    }

    /// Latin-1 maps every char below U+0100 to a single byte, e.g. 'é' becomes 0xE9.
    fn latin1(text: &str) -> Vec<u8> {
        text.chars().map(latin1_byte).collect()
    }

    fn latin1_byte(c: char) -> u8 {
        u8::try_from(u32::from(c)).unwrap_or_else(|_| panic!("'{c}' is not a Latin-1 char"))
    }

    /// Windows-1252 matches Latin-1 except for 0x80-0x9F. Only the chars used by tests are mapped.
    fn windows_1252(text: &str) -> Vec<u8> {
        text.chars()
            .map(|c| match c {
                '€' => 0x80,
                c => latin1_byte(c),
            })
            .collect()
    }

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[rstest]
    #[case::latin1(latin1("/var/log/méxico/app.log"))]
    // '€' is the single byte 0x80 in Windows-1252, never valid on its own in UTF-8
    #[case::windows_1252(windows_1252("/var/log/€/app.log"))]
    #[case::utf16le(utf16le("/var/log/méxico/app.log"))]
    #[traced_test]
    fn spawn_logger_with_non_utf8(#[case] file_name: Vec<u8>) {
        let agent_id = UniqueAgentID::build();
        let mut temp_file = tempfile().unwrap();
        let file_logger = Logger::File(Box::new(FileLogger::new(temp_file.try_clone().unwrap())));

        let output = [
            b"level=info msg=\"Tailing log files\"\n".as_slice(),
            b"level=warning msg=\"Cannot read file\" file=",
            &file_name,
            b"\r\n",
            b"level=info msg=\"Integration started\"\n",
        ]
        .concat();
        let loggers = vec![Logger::Stdout(agent_id.into()), file_logger];

        let (sender_thd, logger_thds) = spawn_logger_inner(std::io::Cursor::new(output), loggers);
        sender_thd
            .join()
            .expect("sender thread should not panic on non-UTF-8 output");
        for thd in logger_thds {
            thd.join().unwrap();
        }

        temp_file.seek(SeekFrom::Start(0)).unwrap();
        let mut content = String::new();
        temp_file.read_to_string(&mut content).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "every input line is forwarded: {lines:?}");
        assert_eq!(lines[0], "level=info msg=\"Tailing log files\"");
        assert!(
            lines[1].starts_with("level=warning msg=\"Cannot read file\" file=")
                && lines[1].contains('\u{FFFD}')
                && !lines[1].ends_with('\r'),
            "invalid bytes replaced and CRLF stripped: {:?}",
            lines[1]
        );
        assert_eq!(lines[2], "level=info msg=\"Integration started\"");
    }

    #[test]
    fn spawn_logger_stops_on_read_error() {
        let mut read_mock = MockRead::new();
        // A retry would call `read` again and panic the sender thread
        read_mock
            .expect_read()
            .once()
            .returning(|_| Err(std::io::Error::other("broken pipe")));

        let agent_id = UniqueAgentID::build();
        let loggers = vec![Logger::Stdout(agent_id.into())];

        let (sender_thd, logger_thds) = spawn_logger_inner(read_mock, loggers);
        sender_thd
            .join()
            .expect("sender thread should stop after the first read error");
        for thd in logger_thds {
            thd.join().unwrap();
        }
    }
}
