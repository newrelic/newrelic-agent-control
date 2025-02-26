use std::{thread, time::Instant};

use opentelemetry::{
    global,
    trace::{TraceContextExt, Tracer},
    InstrumentationScope, KeyValue,
};
use sysinfo::{get_current_pid, System, MINIMUM_CPU_UPDATE_INTERVAL};
use tracing::{error, instrument, warn};

pub fn retrieve_usage_data() {
    let startup_time = Instant::now();

    let pid = get_current_pid().expect("Couldn't get AC's PID");

    let mut sys = System::new_all();

    sys.refresh_all();

    thread::spawn(move || loop {
        wait_for_cpu();

        sys.refresh_all();

        let process_info = sys.process(pid).expect("Couldn't get AC's process info");

        let cpu_usage = process_info.cpu_usage();
        let resident_mem_usage = process_info.memory();
        let virtual_mem_usage = process_info.virtual_memory();

        let uptime_seconds = startup_time.elapsed().as_secs();

        // The special attributes used here (monotonic_counter and histogram) are translated into
        // OTel metrics via the crate `tracing_opentelemetry`. Of course, this is still a `warn`
        // log line, so it will appear in the logs as expected.
        warn!(
            monotonic_counter.uptime_seconds = uptime_seconds,
            agent.name = "agent-control",
            agent._type = "com.newrelic.agent-control",
            agent.version = env!("CARGO_PKG_VERSION")
        );

        warn!(
            histogram.cpu_usage = cpu_usage,
            agent.name = "super-agent",
            agent._type = "super-agent",
            agent.version = env!("CARGO_PKG_VERSION")
        );
        warn!(
            histogram.resident_memory_usage = resident_mem_usage as f64 / (1024.0 * 1024.0),
            agent.name = "super-agent",
            agent._type = "super-agent",
            agent.version = env!("CARGO_PKG_VERSION")
        );
        warn!(
            histogram.virtual_memory_usage = virtual_mem_usage as f64 / (1024.0 * 1024.0),
            agent.name = "super-agent",
            agent._type = "super-agent",
            agent.version = env!("CARGO_PKG_VERSION")
        );

        emit_span();
        emit_metrics();
        emit_log();
    });
}

// The attribute macro should automatically create a span, can be customized (level, etc)
// This is possible by using the `tracing` feature `attributes`, and is converted to an OTel
// trace via the crate `tracing_opentelemetry`
#[instrument]
fn wait_for_cpu() {
    thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
}

// This emit_span creates a span manually. This does not require `tracing_opentelemetry`.
// it's creating the OTel span directly by accessing the configured tracer provider via `global`.
fn emit_span() {
    let scope = InstrumentationScope::builder("agent-control-manual-span")
        .with_version("v1")
        .with_attributes([KeyValue::new("scope_key", "scope_value")])
        .build();

    let tracer = global::tracer_with_scope(scope);
    tracer.in_span("example-span", |cx| {
        let span = cx.span();
        span.set_attribute(KeyValue::new("my-attribute", "my-value"));
        span.add_event(
            "example-event-name",
            vec![KeyValue::new("event_attribute1", "event_value1")],
        );
    })
}

// This emit metrics creates the OTel metrics directly, without using `tracing_opentelemetry`.
// Does so by accessing the configured meter provider via `global`.
fn emit_metrics() {
    let meter = global::meter("agent-control-metrics");
    let c = meter.u64_counter("example-counter").build();
    c.add(
        1,
        &[
            KeyValue::new("name", "apple"),
            KeyValue::new("color", "green"),
        ],
    );
    c.add(
        1,
        &[
            KeyValue::new("name", "apple"),
            KeyValue::new("color", "green"),
        ],
    );
    c.add(
        2,
        &[
            KeyValue::new("name", "apple"),
            KeyValue::new("color", "red"),
        ],
    );
    c.add(
        1,
        &[
            KeyValue::new("name", "banana"),
            KeyValue::new("color", "yellow"),
        ],
    );
    c.add(
        11,
        &[
            KeyValue::new("name", "banana"),
            KeyValue::new("color", "yellow"),
        ],
    );

    let h = meter.f64_histogram("example_histogram").build();
    h.record(
        1.0,
        &[
            KeyValue::new("name", "apple"),
            KeyValue::new("color", "green"),
        ],
    );
    h.record(
        1.0,
        &[
            KeyValue::new("name", "apple"),
            KeyValue::new("color", "green"),
        ],
    );
    h.record(
        2.0,
        &[
            KeyValue::new("name", "apple"),
            KeyValue::new("color", "red"),
        ],
    );
    h.record(
        1.0,
        &[
            KeyValue::new("name", "banana"),
            KeyValue::new("color", "yellow"),
        ],
    );
    h.record(
        11.0,
        &[
            KeyValue::new("name", "banana"),
            KeyValue::new("color", "yellow"),
        ],
    );
}

// Logs work normally thanks to the logging bridge layer configured in the `tracing` initialization.
fn emit_log() {
    error!(name: "my-event-name", target: "my-system", event_id = 20, user_name = "otel", user_email = "otel@opentelemetry.io");
}
