use crate::cgroupfs::Cgroup;
use crate::dbus::Systemd;
use crate::error::{Error, Result, ctx};
use crate::spawn::{command, spawn};
use std::process::Child;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Baseline: a plain child of the supervisor.
    None,
    /// `pre_exec` moving the child into a cgroup created by hand.
    Cgroup,
    /// `systemd-run --scope` invoked as a command.
    SystemdRun,
    /// Transient scope created over D-Bus for a child held back until it is adopted.
    DbusScope,
    /// Transient service created over D-Bus; systemd forks the process.
    DbusService,
}

impl FromStr for Backend {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "none" => Self::None,
            "cgroup" => Self::Cgroup,
            "systemd-run" => Self::SystemdRun,
            "dbus-scope" => Self::DbusScope,
            "dbus-service" => Self::DbusService,
            other => return Err(Error::new(format!("unknown backend {other}"))),
        })
    }
}

fn unit_name(name: &str, suffix: &str) -> String {
    format!("ac-poc-{name}.{suffix}")
}

pub enum Waiter {
    Child(Child),
    Unit(String),
}

pub struct Started {
    pub pid: u32,
    pub waiter: Waiter,
}

impl Backend {
    pub async fn start(self, name: &str, argv: &[String]) -> Result<Started> {
        match self {
            Self::None => {
                let child = spawn(command(argv)?)?;
                Ok(Started {
                    pid: child.id(),
                    waiter: Waiter::Child(child),
                })
            }
            Self::Cgroup => {
                let cgroup = Cgroup::new(name);
                cgroup.create()?;
                let mut cmd = command(argv)?;
                cgroup.attach_on_spawn(&mut cmd)?;
                let child = spawn(cmd)?;
                Ok(Started {
                    pid: child.id(),
                    waiter: Waiter::Child(child),
                })
            }
            Self::SystemdRun => {
                // The scope is created by systemd-run, which then execs the command, so the PID
                // of the child is the PID of the target process.
                let program = std::env::var("POC_SYSTEMD_RUN")
                    .unwrap_or_else(|_| "/usr/bin/systemd-run".to_string());
                let mut cmd = std::process::Command::new(program);
                cmd.args(["--scope", "--quiet", "--collect", "--unit"])
                    .arg(format!("ac-poc-{name}"))
                    .arg("--")
                    .args(argv);
                let child = spawn(cmd)?;
                Ok(Started {
                    pid: child.id(),
                    waiter: Waiter::Child(child),
                })
            }
            Self::DbusScope => crate::gated::start_scope(&unit_name(name, "scope"), argv).await,
            Self::DbusService => {
                let unit = unit_name(name, "service");
                let systemd = Systemd::connect().await?;
                let pid = systemd.start_service(&unit, argv).await?;
                Ok(Started {
                    pid,
                    waiter: Waiter::Unit(unit),
                })
            }
        }
    }

    pub async fn pids(self, name: &str) -> Result<Vec<u32>> {
        match self {
            Self::None => Err(Error::new("the none backend tracks nothing")),
            Self::Cgroup => Ok(Cgroup::new(name).pids()),
            Self::SystemdRun | Self::DbusScope => Ok(Systemd::connect()
                .await?
                .pids(&unit_name(name, "scope"))
                .await),
            Self::DbusService => Ok(Systemd::connect()
                .await?
                .pids(&unit_name(name, "service"))
                .await),
        }
    }

    pub async fn stop(self, name: &str) -> Result<()> {
        match self {
            Self::None => Err(Error::new("the none backend tracks nothing")),
            Self::Cgroup => Cgroup::new(name).kill_and_remove(),
            Self::SystemdRun | Self::DbusScope => {
                Systemd::connect()
                    .await?
                    .stop(&unit_name(name, "scope"))
                    .await
            }
            Self::DbusService => {
                Systemd::connect()
                    .await?
                    .stop(&unit_name(name, "service"))
                    .await
            }
        }
    }
}

impl Waiter {
    pub async fn wait(self) -> Result<String> {
        match self {
            Self::Child(mut child) => {
                let status = tokio::task::spawn_blocking(move || child.wait())
                    .await
                    .map_err(ctx("joining wait task"))?
                    .map_err(ctx("waiting for child"))?;
                Ok(status.to_string())
            }
            Self::Unit(unit) => {
                Systemd::connect().await?.wait_gone(&unit).await?;
                Ok("unit inactive".to_string())
            }
        }
    }
}
