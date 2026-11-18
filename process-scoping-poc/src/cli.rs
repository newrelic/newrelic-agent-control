use crate::backend::Backend;
use crate::cgroupfs;
use crate::error::{Error, Result};
use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

struct Args {
    backend: Backend,
    name: String,
    command: Vec<String>,
}

fn parse(args: &[String]) -> Result<Args> {
    let mut backend = None;
    let mut name = None;
    let mut command = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--backend" => backend = iter.next().map(|s| s.parse()).transpose()?,
            "--name" => name = iter.next().cloned(),
            "--" => {
                command = iter.by_ref().cloned().collect();
            }
            other => return Err(Error::new(format!("unexpected argument {other}"))),
        }
    }
    Ok(Args {
        backend: backend.ok_or_else(|| Error::new("missing --backend"))?,
        name: name.ok_or_else(|| Error::new("missing --name"))?,
        command,
    })
}

pub fn run(args: Vec<String>) -> ExitCode {
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(args: &[String]) -> Result<()> {
    let (subcommand, rest) = args
        .split_first()
        .ok_or_else(|| Error::new("missing subcommand"))?;
    match subcommand.as_str() {
        "run" => {
            let args = parse(rest)?;
            let started = args.backend.start(&args.name, &args.command)?;
            println!("started name={} pid={}", args.name, started.pid);
            std::io::stdout().flush().ok();
            let status = started.waiter.wait()?;
            println!("exited name={} status={status}", args.name);
            Ok(())
        }
        "pids" => {
            let args = parse(rest)?;
            for pid in args.backend.pids(&args.name)? {
                println!("{pid}");
            }
            Ok(())
        }
        "stop" => {
            let args = parse(rest)?;
            args.backend.stop(&args.name)
        }
        "probe" => {
            // First action of the process: record the cgroups it starts in.
            let out = rest.first().ok_or_else(|| Error::new("missing file"))?;
            let cgroup = std::fs::read_to_string("/proc/self/cgroup")
                .map_err(|e| Error::new(format!("reading /proc/self/cgroup: {e}")))?;
            std::fs::write(out, cgroup).map_err(|e| Error::new(format!("writing {out}: {e}")))?;
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        "layout" => {
            println!("{:?}", cgroupfs::detect());
            Ok(())
        }
        other => Err(Error::new(format!("unknown subcommand {other}"))),
    }
}
