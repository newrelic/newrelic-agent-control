use std::time::Duration;
use std::{collections::HashMap, sync::mpsc::Receiver, thread};

use newrelic_super_agent::sub_agent::logger::{AgentLog, LogOutput};
use newrelic_super_agent::sub_agent::on_host::command::command::{
    CommandTerminator, NotStartedCommand, StartedCommand,
};
use newrelic_super_agent::sub_agent::on_host::command::command_os::{CommandOS, NotStarted};
use newrelic_super_agent::sub_agent::on_host::command::logging::file_logger::FileAppender;
use newrelic_super_agent::sub_agent::on_host::command::shutdown::ProcessTerminator;

const TICKER: &str = "test/on_host/command/scripts/ticker.sh";
const TICKER_STDERR: &str = "test/on_host/command/scripts/ticker_stderr.sh";
const TICKER_10: &str = "test/on_host/command/scripts/ticker_10.sh";

// non blocking supervisor
struct NonSupervisor<C = CommandOS<NotStarted>>
where
    C: NotStartedCommand,
{
    cmd: C,
}

fn get_n_outputs(rx: Receiver<AgentLog>, times: usize) -> (Vec<String>, Vec<String>) {
    // stream the actual output on a separate thread
    let stream = thread::spawn(move || {
        let mut stdout_actual = Vec::new();
        let mut stderr_actual = Vec::new();

        let mut match_agent_log = |event: AgentLog| {
            match event.output {
                LogOutput::Stdout(line) => stdout_actual.push(line),
                LogOutput::Stderr(line) => stderr_actual.push(line),
            };
            Some(())
        };

        if times == 0 {
            (0..).try_for_each(|_| match_agent_log(rx.recv().ok()?))
        } else {
            (0..times).try_for_each(|_| match_agent_log(rx.recv().ok()?))
        };

        (stdout_actual, stderr_actual)
    });

    // wait for the process to finish
    stream.join().unwrap()
}

#[test]
fn actual_command_streaming() {
    let agent_id = "ticker-test".to_owned().try_into().unwrap();
    let agent = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new(
            agent_id,
            "sh",
            [TICKER],
            HashMap::from([("TEST", "TEST")]),
            false,
        ),
    };

    let (tx, rx) = std::sync::mpsc::channel();

    let streaming_runner = agent.cmd.start().unwrap().stream(tx).unwrap();

    // Populate the expected output
    let mut stdout_expected = Vec::new();
    let mut stderr_expected = Vec::new();
    (0..10).for_each(|i| stdout_expected.push(format!("ok tick {}", i)));
    (0..10).for_each(|i| stderr_expected.push(format!("err tick {}", i)));

    // wait for the process to finish
    let (stdout_actual, stderr_actual) = get_n_outputs(rx, 20);

    assert_eq!(stdout_expected, stdout_actual);
    assert_eq!(stderr_expected, stderr_actual);

    // kill the process
    #[cfg(unix)]
    {
        let terminated = ProcessTerminator::new(streaming_runner.get_pid()).shutdown(|| true);
        assert!(terminated.is_ok());
    }
}

#[test]
fn actual_command_streaming_file_loggers() {
    let agent_id = "ticker-test".to_owned().try_into().unwrap();
    // Let's create some file loggers
    let temp_dir = tempfile::tempdir().unwrap();
    let file_logger_out =
        FileAppender::new_with_fixed_file(&agent_id, temp_dir.as_ref(), "stdout.log").into();
    let file_logger_err =
        FileAppender::new_with_fixed_file(&agent_id, temp_dir.as_ref(), "stderr.log").into();

    let agent = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new(
            agent_id,
            "sh",
            [TICKER],
            HashMap::from([("TEST", "TEST")]),
            true,
        ),
    };

    let (tx, rx) = std::sync::mpsc::channel();

    let streaming_runner = agent
        .cmd
        .start_with_loggers(file_logger_out, file_logger_err)
        .unwrap()
        .stream(tx)
        .unwrap();

    // Populate the expected output
    let mut stdout_expected = Vec::new();
    let mut stderr_expected = Vec::new();
    (0..10).for_each(|i| stdout_expected.push(format!("ok tick {}", i)));
    (0..10).for_each(|i| stderr_expected.push(format!("err tick {}", i)));

    // wait for the process to finish
    let (stdout_actual, stderr_actual) = get_n_outputs(rx, 20);

    assert_eq!(stdout_expected, stdout_actual);
    assert_eq!(stderr_expected, stderr_actual);

    // kill the process
    #[cfg(unix)]
    {
        let terminated = ProcessTerminator::new(streaming_runner.get_pid()).shutdown(|| true);
        assert!(terminated.is_ok());
    }

    // After flushing the file loggers, which happens on termination,
    // the files should contain the expected contents

    // Normally the below code would suffice, but slower systems (containers) might need a bit more
    // time for the file to be written after the process terminates. Let's wait for a second here.
    thread::sleep(Duration::from_secs(1));

    // Create the expected contents of the file loggers
    let expected_stdout_file = stdout_expected.join("\n") + "\n";
    let expected_stderr_file = stderr_expected.join("\n") + "\n";

    // Read the actual files
    let actual_stdout_file =
        std::fs::read_to_string(temp_dir.path().join("ticker-test").join("stdout.log")).unwrap();
    let actual_stderr_file =
        std::fs::read_to_string(temp_dir.path().join("ticker-test").join("stderr.log")).unwrap();

    assert_eq!(expected_stdout_file, actual_stdout_file);
    assert_eq!(expected_stderr_file, actual_stderr_file);
}

#[test]
fn many_cmds_to_same_file_logger() {
    let agent_id = "ticker-test".to_owned().try_into().unwrap();
    // Let's create some file loggers
    let temp_dir = tempfile::tempdir().unwrap();
    let file_logger_out_1 =
        FileAppender::new_with_fixed_file(&agent_id, temp_dir.as_ref(), "stdout.log").into();
    let file_logger_err_1 =
        FileAppender::new_with_fixed_file(&agent_id, temp_dir.as_ref(), "stderr.log").into();

    let file_logger_out_2 =
        FileAppender::new_with_fixed_file(&agent_id, temp_dir.as_ref(), "stdout.log").into();
    let file_logger_err_2 =
        FileAppender::new_with_fixed_file(&agent_id, temp_dir.as_ref(), "stderr.log").into();

    let agent_1 = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new(
            agent_id.clone(),
            "sh",
            [TICKER_10],
            HashMap::from([("TEST", "TEST")]),
            true,
        ),
    };

    let agent_2 = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new(
            agent_id,
            "sh",
            [TICKER_10],
            HashMap::from([("TEST", "TEST")]),
            true,
        ),
    };

    let (tx, rx) = std::sync::mpsc::channel();

    let streaming_runner_1 = agent_1
        .cmd
        .start_with_loggers(file_logger_out_1, file_logger_err_1)
        .unwrap()
        .stream(tx.clone())
        .unwrap();

    let streaming_runner_2 = agent_2
        .cmd
        .start_with_loggers(file_logger_out_2, file_logger_err_2)
        .unwrap()
        .stream(tx)
        .unwrap();

    // Populate the expected output
    let mut stdout_expected = Vec::new();
    let mut stderr_expected = Vec::new();
    (0..10).for_each(|i| {
        let msg = format!("ok tick {}", i);
        stdout_expected.push(msg.clone());
        stdout_expected.push(msg);
    });
    (0..10).for_each(|i| {
        let msg = format!("err tick {}", i);
        stderr_expected.push(msg.clone());
        stderr_expected.push(msg);
    });

    // wait for the process to finish
    let (stdout_actual, stderr_actual) = get_n_outputs(rx, 40);

    assert_eq!(stdout_expected, stdout_actual);
    assert_eq!(stderr_expected, stderr_actual);

    // kill the process
    #[cfg(unix)]
    {
        let terminated = ProcessTerminator::new(streaming_runner_1.get_pid()).shutdown(|| true);
        assert!(terminated.is_ok());

        let terminated = ProcessTerminator::new(streaming_runner_2.get_pid()).shutdown(|| true);
        assert!(terminated.is_ok());
    }

    // After flushing the file loggers, which happens on termination,
    // the files should contain the expected contents

    // The order is unknown, so we sort the expected contents
    let mut expected_stdout_file = stdout_expected;
    let mut expected_stderr_file = stderr_expected;
    expected_stdout_file.sort();
    expected_stderr_file.sort();

    // Read the actual files, store them in vector and sort them in the same way as the expected ones.
    let mut actual_stdout_file =
        std::fs::read_to_string(temp_dir.path().join("ticker-test").join("stdout.log"))
            .unwrap()
            .lines()
            .map(String::from)
            .collect::<Vec<_>>();
    actual_stdout_file.sort();
    let mut actual_stderr_file =
        std::fs::read_to_string(temp_dir.path().join("ticker-test").join("stderr.log"))
            .unwrap()
            .lines()
            .map(String::from)
            .collect::<Vec<_>>();
    actual_stderr_file.sort();

    assert_eq!(expected_stdout_file, actual_stdout_file);
    assert_eq!(expected_stderr_file, actual_stderr_file);
}

#[test]
fn actual_command_streaming_only_stderr() {
    let agent_id = "ticker-test".to_owned().try_into().unwrap();
    let agent = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new(
            agent_id,
            "sh",
            [TICKER_STDERR],
            HashMap::from([("TEST", "TEST")]),
            false,
        ),
    };

    let (tx, rx) = std::sync::mpsc::channel();

    let streaming_runner = agent.cmd.start().unwrap().stream(tx).unwrap();

    // Populate the expected output
    let stdout_expected: Vec<String> = Vec::new();
    let mut stderr_expected: Vec<String> = Vec::new();
    (0..10).for_each(|i| stderr_expected.push(format!("err tick {}", i)));

    // wait for the process to finish
    let (stdout_actual, stderr_actual) = get_n_outputs(rx, 10);

    assert_eq!(stdout_expected, stdout_actual);
    assert_eq!(stderr_expected, stderr_actual);

    // kill the process
    #[cfg(unix)]
    {
        let terminated = ProcessTerminator::new(streaming_runner.get_pid()).shutdown(|| true);
        assert!(terminated.is_ok());
    }
}

#[test]
fn actual_command_exiting_closes_channel() {
    let agent_id = "ticker-test".to_owned().try_into().unwrap();
    let agent = NonSupervisor {
        // TICKER_10 actually exits when it has ticked 10 times both on stdout and stderr
        cmd: CommandOS::<NotStarted>::new(
            agent_id,
            "sh",
            [TICKER_10],
            HashMap::from([("TEST", "TEST")]),
            false,
        ),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    // Start streaming (NOTE the use of handle on the last line)
    let handle = agent.cmd.start().unwrap().stream(tx).unwrap();
    // Populate the expected output
    let mut stdout_expected = Vec::new();
    let mut stderr_expected = Vec::new();

    (0..10).for_each(|i| stdout_expected.push(format!("ok tick {}", i)));
    (0..10).for_each(|i| stderr_expected.push(format!("err tick {}", i)));

    // wait for the thread loop to break
    let (stdout_actual, stderr_actual) = get_n_outputs(rx, 0);

    assert_eq!(stdout_expected, stdout_actual);
    assert_eq!(stderr_expected, stderr_actual);

    // At this point, the process can be terminated because the process exited on its own
    #[cfg(unix)]
    {
        let terminated = ProcessTerminator::new(handle.get_pid()).shutdown(|| true);
        assert!(terminated.is_ok());
    }
}

#[test]
fn env_vars_are_inherited() {
    // Set environment variable
    std::env::set_var("FOO", "bar");
    std::env::set_var("BAR", "baz");

    // Child processes will inherit environment variables from their parent process by default
    let agent_id = "env-test".to_owned().try_into().unwrap();
    let agent = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new(
            agent_id,
            "sh",
            ["-c", "echo $FOO; echo $BAR"],
            HashMap::from([("TEST", "TEST")]),
            false,
        ),
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let _handle = agent.cmd.start().unwrap().stream(tx).unwrap();

    let expected = vec!["bar", "baz"];

    let (stdout_actual, _stderr_actual) = get_n_outputs(rx, 2);

    assert_eq!(expected, stdout_actual);
}
