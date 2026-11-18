//! Backends C and D: transient units created over the systemd D-Bus API with `zbus_systemd`.
use crate::error::{Error, Result, ctx};
use std::time::Duration;
use tokio_stream::StreamExt;
use zbus_systemd::systemd1::{ManagerProxy, ServiceProxy, UnitProxy};
use zbus_systemd::zbus::{Connection, OwnedVariant, Variant};

type Properties = Vec<(String, OwnedVariant)>;

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
    pub async fn connect() -> Result<Self> {
        let connection = Connection::system()
            .await
            .map_err(ctx("connecting to the system bus"))?;
        let manager = ManagerProxy::new(&connection)
            .await
            .map_err(ctx("creating the systemd manager proxy"))?;
        manager.subscribe().await.map_err(ctx("subscribing"))?;
        Ok(Self {
            connection,
            manager,
        })
    }

    /// Calls `StartTransientUnit` and waits for its job to finish.
    async fn start_transient(&self, unit: &str, properties: Properties) -> Result<()> {
        let mut removed = self
            .manager
            .receive_job_removed()
            .await
            .map_err(ctx("subscribing to job removals"))?;
        let job = self
            .manager
            .start_transient_unit(unit.to_string(), "fail".to_string(), properties, Vec::new())
            .await
            .map_err(ctx("starting transient unit"))?;
        let wait = async {
            while let Some(signal) = removed.next().await {
                let args = signal.args().map_err(ctx("reading job signal"))?;
                if *args.job() == job {
                    return match args.result().as_str() {
                        "done" => Ok(()),
                        other => Err(Error::new(format!("job for {unit} ended with {other}"))),
                    };
                }
            }
            Err(Error::new("job signal stream ended"))
        };
        tokio::time::timeout(Duration::from_secs(30), wait)
            .await
            .map_err(|_| Error::new(format!("timeout waiting for the job of {unit}")))?
    }

    /// Transient scope adopting `pid`.
    pub async fn start_scope(&self, unit: &str, pid: u32) -> Result<()> {
        let properties = vec![
            ("Description".to_string(), variant("process-scoping-poc")?),
            ("PIDs".to_string(), variant(vec![pid])?),
        ];
        self.start_transient(unit, properties).await
    }

    /// Transient service that systemd forks itself; returns the main PID.
    pub async fn start_service(&self, unit: &str, argv: &[String]) -> Result<u32> {
        let program = argv.first().ok_or_else(|| Error::new("empty command"))?;
        let exec_start = vec![(program.clone(), argv.to_vec(), false)];
        let properties = vec![
            ("Description".to_string(), variant("process-scoping-poc")?),
            ("Type".to_string(), variant("simple")?),
            ("ExecStart".to_string(), variant(exec_start)?),
        ];
        self.start_transient(unit, properties).await?;
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
    }

    /// PIDs in the cgroup of the unit; empty if the unit does not exist.
    pub async fn pids(&self, unit: &str) -> Vec<u32> {
        match self.manager.get_unit_processes(unit.to_string()).await {
            Ok(processes) => processes.into_iter().map(|(_, pid, _)| pid).collect(),
            Err(_) => Vec::new(),
        }
    }

    pub async fn stop(&self, unit: &str) -> Result<()> {
        self.manager
            .stop_unit(unit.to_string(), "replace".to_string())
            .await
            .map_err(ctx("stopping unit"))?;
        for _ in 0..100 {
            if self.pids(unit).await.is_empty() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Error::new(format!("{unit} still has processes after stop")))
    }

    /// Returns once the unit is gone or inactive.
    pub async fn wait_gone(&self, unit: &str) -> Result<()> {
        loop {
            let Ok(path) = self.manager.get_unit(unit.to_string()).await else {
                return Ok(());
            };
            let proxy = UnitProxy::builder(&self.connection)
                .path(path)
                .build()
                .await
                .map_err(ctx("building unit proxy"))?;
            match proxy.active_state().await.as_deref() {
                Ok("inactive" | "failed") | Err(_) => return Ok(()),
                Ok(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }
}
