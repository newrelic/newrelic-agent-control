use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::install::tear_down_test;
use crate::{
    common::nrql,
    linux::{self, install::install_agent_control_from_recipe},
};
use std::net::UdpSocket;
use std::time::Duration;
use tracing::info;

pub fn test_nrdot_agent(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        monitoring_source: "network".to_string(),
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);
    let test_id = format!(
        "onhost-e2e-network-flow_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setup Agent Control config with network flow monitoring agent");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nrdot:
    agent_type: newrelic/com.newrelic.opentelemetry.collector:0.1.0
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path("nrdot"),
        format!(
            r#"
nr_account_id: "{}"
container_name: "ktranslate-flow-e2e"
"#,
            recipe_data.args.nr_account_id
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    // Wait for ktranslate container to be ready, then send synthetic NetFlow v5 packets
    info!("Waiting for ktranslate to start before sending synthetic flow data");
    std::thread::sleep(Duration::from_secs(30));
    send_synthetic_netflow_packets(9995);

    let nrql_query = format!(r#"SELECT * FROM KFlow WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        // Keep sending flow packets on each retry to ensure data arrives
        send_synthetic_netflow_packets(9995);
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}

/// Sends a valid NetFlow v5 UDP packet to localhost:port so ktranslate has data to process.
fn send_synthetic_netflow_packets(port: u16) {
    // A minimal valid NetFlow v5 packet: 24-byte header + one 48-byte flow record.
    // Represents a single TCP flow from 10.0.1.1:8080 -> 10.0.2.1:443.
    #[rustfmt::skip]
    let packet: &[u8] = &[
        // Header: version=5, count=1, uptime/timestamps/sequence (zeroed is valid)
        0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        // Flow record:
        0x0a, 0x00, 0x01, 0x01, // src: 10.0.1.1
        0x0a, 0x00, 0x02, 0x01, // dst: 10.0.2.1
        0x0a, 0x00, 0x00, 0x01, // next_hop: 10.0.0.1
        0x00, 0x01, 0x00, 0x02, // input/output iface
        0x00, 0x00, 0x00, 0x64, // packets: 100
        0x00, 0x00, 0x3a, 0x98, // bytes: 15000
        0x00, 0x00, 0x00, 0x00, // first
        0x00, 0x00, 0x00, 0x00, // last
        0x1f, 0x90, 0x01, 0xbb, // src_port: 8080, dst_port: 443
        0x00, 0x1b, 0x06, 0x00, // pad, tcp_flags, proto=TCP, tos
        0xfb, 0xf4, 0x3b, 0x41, // src_as: 64500, dst_as: 15169
        0x18, 0x18, 0x00, 0x00, // src/dst mask: /24, pad
    ];

    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        info!("Failed to bind UDP socket for synthetic flow");
        return;
    };

    let addr = format!("127.0.0.1:{port}");
    for _ in 0..3 {
        let _ = sock.send_to(packet, &addr);
        std::thread::sleep(Duration::from_millis(500));
    }
    info!("Sent synthetic NetFlow v5 packets to {addr}");
}
