//! Backends C and D: transient units created over the systemd D-Bus API with `zbus_systemd`.
//!
//! zbus 6 has no blocking API, so each call is driven to completion with `block_on`; zbus does its
//! I/O on its own runtime threads.
use crate::error::{Error, Result, ctx};
use futures_executor::block_on;
use std::thread::sleep;
use std::time::{Duration, Instant};
use zbus_systemd::systemd1::{ManagerProxy, ServiceProxy, UnitProxy};
use zbus_systemd::zbus::{Connection, OwnedVariant, Variant};

type Properties = Vec<(String, OwnedVariant)>;

const JOB_TIMEOUT: Duration = Duration::from_secs(30);

fn variant<T>(value: T) -> Result<OwnedVariant>
where
    T: Into<Variant<'static>> + zbus_systemd::zbus::DynamicType,
{
    OwnedVariant::try_from(Variant::new(value)).map_err(ctx("building property"))
}

pub struct Systemd {
    connection: Connection,
    manager: ManagerProxy<'static>,
}

impl Systemd {
    pub fn connect() -> Result<Self> {
        block_on(async {
            let connection = Connection::system()
                .await
                .map_err(ctx("connecting to the system bus"))?;
            let manager = ManagerProxy::new(&connection)
                .await
                .map_err(ctx("creating the systemd manager proxy"))?;
            Ok(Self {
                connection,
                manager,
            })
        })
    }

    fn unit_proxy(&self, unit: &str) -> Result<Option<UnitProxy<'static>>> {
        block_on(async {
            let Ok(path) = self.manager.get_unit(unit.to_string()).await else {
                return Ok(None);
            };
            UnitProxy::builder(&self.connection)
                .path(path)
                .build()
                .await
                .map(Some)
                .map_err(ctx("building unit proxy"))
        })
    }

    fn active_state(&self, unit: &str) -> Option<String> {
        let proxy = self.unit_proxy(unit).ok()??;
        block_on(proxy.active_state()).ok()
    }

    /// Calls `StartTransientUnit` and waits until the unit is active.
    fn start_transient(&self, unit: &str, properties: Properties) -> Result<()> {
        block_on(self.manager.start_transient_unit(
            unit.to_string(),
            "fail".to_string(),
            properties,
            Vec::new(),
        ))
        .map_err(ctx("starting transient unit"))?;
        let deadline = Instant::now() + JOB_TIMEOUT;
        loop {
            match self.active_state(unit).as_deref() {
                Some("active") => return Ok(()),
                Some("failed") => return Err(Error::new(format!("{unit} failed to start"))),
                _ if Instant::now() > deadline => {
                    return Err(Error::new(format!("timeout waiting for {unit}")));
                }
                _ => sleep(Duration::from_millis(50)),
            }
        }
    }

    /// Transient scope adopting `pid`.
    pub fn start_scope(&self, unit: &str, pid: u32) -> Result<()> {
        let properties = vec![
            ("Description".to_string(), variant("process-scoping-poc")?),
            ("PIDs".to_string(), variant(vec![pid])?),
        ];
        self.start_transient(unit, properties)
    }

    /// Transient service that systemd forks itself; returns the main PID.
    pub fn start_service(&self, unit: &str, argv: &[String]) -> Result<u32> {
        let program = argv.first().ok_or_else(|| Error::new("empty command"))?;
        let exec_start = vec![(program.clone(), argv.to_vec(), false)];
        let properties = vec![
            ("Description".to_string(), variant("process-scoping-poc")?),
            ("Type".to_string(), variant("simple")?),
            ("ExecStart".to_string(), variant(exec_start)?),
        ];
        self.start_transient(unit, properties)?;
        block_on(async {
            let path = self
                .manager
                .get_unit(unit.to_string())
                .await
                .map_err(ctx("getting unit"))?;
            let service = ServiceProxy::builder(&self.connection)
                .path(path)
                .build()
                .await
                .map_err(ctx("building service proxy"))?;
            service.main_pid().await.map_err(ctx("reading main pid"))
        })
    }

    /// PIDs in the cgroup of the unit; empty if the unit does not exist.
    pub fn pids(&self, unit: &str) -> Vec<u32> {
        match block_on(self.manager.get_unit_processes(unit.to_string())) {
            Ok(processes) => processes.into_iter().map(|(_, pid, _)| pid).collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn stop(&self, unit: &str) -> Result<()> {
        block_on(
            self.manager
                .stop_unit(unit.to_string(), "replace".to_string()),
        )
        .map_err(ctx("stopping unit"))?;
        for _ in 0..100 {
            if self.pids(unit).is_empty() {
                return Ok(());
            }
            sleep(Duration::from_millis(100));
        }
        Err(Error::new(format!("{unit} still has processes after stop")))
    }

    /// Returns once the unit is gone or inactive.
    pub fn wait_gone(&self, unit: &str) -> Result<()> {
        while !matches!(
            self.active_state(unit).as_deref(),
            None | Some("inactive" | "failed")
        ) {
            sleep(Duration::from_millis(200));
        }
        Ok(())
    }
}
