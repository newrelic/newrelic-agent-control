use std::{collections::HashMap, sync::mpsc::Receiver, thread};

use newrelic_super_agent::sub_agent::logger::{Event, OutputEvent};
use newrelic_super_agent::sub_agent::on_host::command::command::{
    CommandTerminator, NotStartedCommand, StartedCommand,
};
use newrelic_super_agent::sub_agent::on_host::command::command_os::NotStartedCommandOS;
use newrelic_super_agent::sub_agent::on_host::command::shutdown::ProcessTerminator;

const TICKER: &str = "test/on_host/command/scripts/ticker.sh";
const TICKER_STDERR: &str = "test/on_host/command/scripts/ticker_stderr.sh";
const TICKER_10: &str = "test/on_host/command/scripts/ticker_10.sh";

// non blocking supervisor
struct NonSupervisor<C = NotStartedCommandOS>
where
    C: NotStartedCommand,
{
    cmd: C,
}

fn get_n_outputs(rx: Receiver<Event>, times: usize) -> (Vec<String>, Vec<String>) {
    // stream the actual output on a separate thread
    let stream = thread::spawn(move || {
        let mut stdout_actual = Vec::new();
        let mut stderr_actual = Vec::new();

        let mut match_output_event = |event: Event| {
            match event.output {
                OutputEvent::Stdout(line) => stdout_actual.push(line),
                OutputEvent::Stderr(line) => stderr_actual.push(line),
            };
            Some(())
        };

        if times == 0 {
            (0..).try_for_each(|_| match_output_event(rx.recv().ok()?))
        } else {
            (0..times).try_for_each(|_| match_output_event(rx.recv().ok()?))
        };

        (stdout_actual, stderr_actual)
    });

    // wait for the process to finish
    stream.join().unwrap()
}

#[test]
fn actual_command_streaming() {
    let agent = NonSupervisor {
        cmd: NotStartedCommandOS::new("sh", [TICKER], HashMap::from([("TEST", "TEST")])),
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
fn actual_command_streaming_only_stderr() {
    let agent = NonSupervisor {
        cmd: NotStartedCommandOS::new("sh", [TICKER_STDERR], HashMap::from([("TEST", "TEST")])),
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
    let agent = NonSupervisor {
        // TICKER_10 actually exits when it has ticked 10 times both on stdout and stderr
        cmd: NotStartedCommandOS::new("sh", [TICKER_10], HashMap::from([("TEST", "TEST")])),
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
    let agent = NonSupervisor {
        cmd: NotStartedCommandOS::new(
            "sh",
            ["-c", "echo $FOO; echo $BAR"],
            HashMap::from([("TEST", "TEST")]),
        ),
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let _handle = agent.cmd.start().unwrap().stream(tx).unwrap();

    let expected = vec!["bar", "baz"];

    let (stdout_actual, _stderr_actual) = get_n_outputs(rx, 2);

    assert_eq!(expected, stdout_actual);
}
