use std::{
    thread,
    time::{Duration, Instant},
};

use sysinfo::{
    get_current_pid, Pid, ProcessRefreshKind, RefreshKind, System, MINIMUM_CPU_UPDATE_INTERVAL,
};
use tracing::warn;

pub fn retrieve_usage_data(pid: Option<u32>) {
    let startup_time = Instant::now();

    let pid = match pid {
        Some(pid) => Pid::from_u32(pid),
        None => get_current_pid().expect("Could not get current PID"),
    };

    // System info retrieval structure
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new().with_memory().with_cpu()),
        // .with_memory(MemoryRefreshKind::everything())
        // .with_cpu(CpuRefreshKind::everything()),
    );

    thread::spawn(move || {
        loop {
            thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);

            // Update memory info
            // sys.refresh_memory();
            // sys.refresh_cpu();
            sys.refresh_process_specifics(pid, ProcessRefreshKind::new().with_memory().with_cpu());

            let process_info = sys.process(pid).expect("Could not get process info");

            let cpu_usage = process_info.cpu_usage();
            let resident_memory_usage = process_info.memory();
            let virtual_memory_usage = process_info.virtual_memory();

            let uptime_seconds = startup_time.elapsed().as_secs();

            warn!(
                monotonic_counter.uptime = uptime_seconds,
                agent.name = "super-agent",
                agent._type = "super-agent",
                agent.version = "0.10.0"
            );
            warn!(
                histogram.cpu_usage = cpu_usage,
                agent.name = "super-agent",
                agent._type = "super-agent",
                agent.version = "0.10.0"
            );
            warn!(
                histogram.resident_memory_usage = resident_memory_usage as f64 / (1024.0 * 1024.0),
                agent.name = "super-agent",
                agent._type = "super-agent",
                agent.version = "0.10.0"
            );
            warn!(
                histogram.virtual_memory_usage = virtual_memory_usage as f64 / (1024.0 * 1024.0),
                agent.name = "super-agent",
                agent._type = "super-agent",
                agent.version = "0.10.0"
            );

            // Let's print it for now:
            warn!(
                "CPU: {:.2}%, Resident Memory: {:.2} MB, Virtual Memory: {:.2} MB, Uptime: {} seconds",
                cpu_usage,
                resident_memory_usage as f64 / (1024.0 * 1024.0),
                virtual_memory_usage as f64 / (1024.0 * 1024.0),
                uptime_seconds);

            // todo!("Send metrics");

            // Sleep for 1 second
            thread::sleep(Duration::from_secs(1));
        }
    });
}
