use std::thread;

use meta_agent::command::{
    stream::OutputEvent, wrapper::ProcessRunner, CommandExecutor, CommandHandle, OutputStreamer,
};

const TICKER: &str = "tests/command/scripts/ticker.sh";
const TICKER_10: &str = "tests/command/scripts/ticker_10.sh";

// non blocking supervisor
struct NonSupervisor<C = ProcessRunner>
where
    C: CommandExecutor,
{
    cmd: C,
}

#[test]
fn actual_command_streaming() {
    let agent = NonSupervisor {
        cmd: ProcessRunner::new("sh", [TICKER]),
    };

    let (tx, rx) = std::sync::mpsc::channel();

    let streaming_cmd = agent.cmd.start().unwrap().stream(tx).unwrap();

    // Populate the expected output
    let mut stdout_expected = Vec::new();
    let mut stderr_expected = Vec::new();
    (0..10).for_each(|i| stdout_expected.push(format!("ok tick {}", i)));
    (0..10).for_each(|i| stderr_expected.push(format!("err tick {}", i)));

    // stream the actual output on a separate thread
    let stream = thread::spawn(move || {
        let mut stdout_actual = Vec::new();
        let mut stderr_actual = Vec::new();

        (0..20).for_each(|_| match rx.recv().unwrap() {
            OutputEvent::Stdout(line) => {
                stdout_actual.push(line);
            }
            OutputEvent::Stderr(line) => {
                stderr_actual.push(line);
            }
        });

        (stdout_actual, stderr_actual)
    });

    // wait for the process to finish
    let (stdout_actual, stderr_actual) = stream.join().unwrap();

    assert_eq!(stdout_expected, stdout_actual);
    assert_eq!(stderr_expected, stderr_actual);

    // kill the process
    assert_eq!(streaming_cmd.stop().is_err(), false);
}

#[test]
fn actual_command_exiting_closes_channel() {
    let agent = NonSupervisor {
        // TICKER_10 actually exits when it has ticket 10 times both on stdout and stderr
        cmd: ProcessRunner::new("sh", [TICKER_10]),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    // Start streaming (NOTE the use of handle on the last line)
    let handle = agent.cmd.start().unwrap().stream(tx).unwrap();
    // Populate the expected output
    let mut stdout_expected = Vec::new();
    let mut stderr_expected = Vec::new();

    (0..10).for_each(|i| stdout_expected.push(format!("ok tick {}", i)));
    (0..10).for_each(|i| stderr_expected.push(format!("err tick {}", i)));

    // stream the actual output on a separate thread
    let stream = thread::spawn(move || {
        let mut stdout_actual = Vec::new();
        let mut stderr_actual = Vec::new();

        loop {
            match rx.recv() {
                Err(_) => {
                    break;
                }
                Ok(event) => match event {
                    OutputEvent::Stdout(line) => {
                        stdout_actual.push(line);
                    }
                    OutputEvent::Stderr(line) => {
                        stderr_actual.push(line);
                    }
                },
            }
        }

        (stdout_actual, stderr_actual)
    });

    // wait for the thread loop to break
    let (stdout_actual, stderr_actual) = stream.join().unwrap();

    assert_eq!(stdout_expected, stdout_actual);
    assert_eq!(stderr_expected, stderr_actual);

    // At this point, the handle can be closed because the process exited on its own!
    #[cfg(unix)]
    assert_eq!(handle.stop().is_err(), false);

    // But...
    // FIXME: ???
    #[cfg(windows)]
    assert_eq!(handle.stop().is_err(), true);
}
