//! Backends C and D: transient units created over the systemd D-Bus API with `zbus_systemd`.
//!
//! zbus has no blocking API, so each operation is one `zbus::block_on` call that connects, calls
//! systemd and drops the connection before returning. A connection that outlives `block_on` gets
//! a helper thread of its own, so none is kept between operations; waiting is done by the callers
//! with `std::thread::sleep`.
use crate::error::{Error, Result, ctx};
use std::thread::sleep;
use std::time::{Duration, Instant};
use zbus_systemd::systemd1::{ManagerProxy, ServiceProxy, UnitProxy};
use zbus_systemd::zbus::{Connection, OwnedVariant, Variant, block_on};

type Properties = Vec<(String, OwnedVariant)>;

const JOB_TIMEOUT: Duration = Duration::from_secs(30);

fn variant<T>(value: T) -> Result<OwnedVariant>
where
    T: Into<Variant<'static>> + zbus_systemd::zbus::DynamicType,
{
    OwnedVariant::try_from(Variant::new(value)).map_err(ctx("building property"))
}

/// Runs `operation` on a fresh system bus connection that is gone when this returns.
fn with_manager<T>(
    operation: impl AsyncFnOnce(&Connection, &ManagerProxy<'_>) -> Result<T>,
) -> Result<T> {
    block_on(async {
        let connection = Connection::system()
            .await
            .map_err(ctx("connecting to the system bus"))?;
        let manager = ManagerProxy::new(&connection)
            .await
            .map_err(ctx("creating the systemd manager proxy"))?;
        operation(&connection, &manager).await
    })
}

/// Active state of the unit, `None` if systemd does not know it.
fn active_state(unit: &str) -> Option<String> {
    with_manager(async |connection, manager| {
        let path = manager
            .get_unit(unit.to_string())
            .await
            .map_err(ctx("getting unit"))?;
        let proxy = UnitProxy::builder(connection)
            .path(path)
            .build()
            .await
            .map_err(ctx("building unit proxy"))?;
        proxy.active_state().await.map_err(ctx("reading state"))
    })
    .ok()
}

/// Calls `StartTransientUnit` and waits until the unit is active.
fn start_transient(unit: &str, properties: Properties) -> Result<()> {
    with_manager(async |_, manager| {
        manager
            .start_transient_unit(unit.to_string(), "fail".to_string(), properties, Vec::new())
            .await
            .map_err(ctx("starting transient unit"))
    })?;
    let deadline = Instant::now() + JOB_TIMEOUT;
    loop {
        match active_state(unit).as_deref() {
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
pub fn start_scope(unit: &str, pid: u32) -> Result<()> {
    let properties = vec![
        ("Description".to_string(), variant("process-scoping-poc")?),
        ("PIDs".to_string(), variant(vec![pid])?),
    ];
    start_transient(unit, properties)
}

/// Transient service that systemd forks itself; returns the main PID.
pub fn start_service(unit: &str, argv: &[String]) -> Result<u32> {
    let program = argv.first().ok_or_else(|| Error::new("empty command"))?;
    let exec_start = vec![(program.clone(), argv.to_vec(), false)];
    let properties = vec![
        ("Description".to_string(), variant("process-scoping-poc")?),
        ("Type".to_string(), variant("simple")?),
        ("ExecStart".to_string(), variant(exec_start)?),
    ];
    start_transient(unit, properties)?;
    with_manager(async |connection, manager| {
        let path = manager
            .get_unit(unit.to_string())
            .await
            .map_err(ctx("getting unit"))?;
        let service = ServiceProxy::builder(connection)
            .path(path)
            .build()
            .await
            .map_err(ctx("building service proxy"))?;
        service.main_pid().await.map_err(ctx("reading main pid"))
    })
}

/// PIDs in the cgroup of the unit; empty if the unit does not exist.
pub fn pids(unit: &str) -> Vec<u32> {
    with_manager(async |_, manager| {
        manager
            .get_unit_processes(unit.to_string())
            .await
            .map_err(ctx("listing processes"))
    })
    .map(|processes| processes.into_iter().map(|(_, pid, _)| pid).collect())
    .unwrap_or_default()
}

pub fn stop(unit: &str) -> Result<()> {
    with_manager(async |_, manager| {
        manager
            .stop_unit(unit.to_string(), "replace".to_string())
            .await
            .map(drop)
            .map_err(ctx("stopping unit"))
    })?;
    for _ in 0..100 {
        if pids(unit).is_empty() {
            return Ok(());
        }
        sleep(Duration::from_millis(100));
    }
    Err(Error::new(format!("{unit} still has processes after stop")))
}

/// Returns once the unit is gone or inactive.
pub fn wait_gone(unit: &str) {
    while !matches!(
        active_state(unit).as_deref(),
        None | Some("inactive" | "failed")
    ) {
        sleep(Duration::from_millis(200));
    }
}
