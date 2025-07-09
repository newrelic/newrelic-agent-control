use assert_cmd::assert::Assert;
use std::io::{self, Write};

pub fn print_cli_output(assert: &Assert) {
    let output = assert.get_output();
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
}

pub fn assert_stdout_contains(assert: &Assert, value: &str) {
    let stderr = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stderr.contains(value))
}
