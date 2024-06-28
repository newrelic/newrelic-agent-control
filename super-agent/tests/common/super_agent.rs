use newrelic_super_agent::cli::{Cli, CliCommand, SuperAgentCliConfig};
use newrelic_super_agent::super_agent::run::SuperAgentRunner;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::MutexGuard;

static SA_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// - The SA relies on Global varialbes super_agent::defaults::* so we need to syncronize
/// the access to this function.
/// - Do not attempt to use defualt vars super_agent::defaults::* before the super-agent initialization since
/// it could lead to the --debug feature not work as expected
pub fn init_sa<'a>(
    debug_dir: &'a Path,
    sa_config: &'a str,
) -> (SuperAgentCliConfig, MutexGuard<'a, ()>) {
    let config_path = debug_dir.join("config.yml");

    File::create(&config_path)
        .unwrap()
        .write_all(sa_config.as_bytes())
        .unwrap();

    let args = vec![
        "",
        "--config",
        config_path.to_str().unwrap(),
        "--debug",
        debug_dir.to_str().unwrap(),
    ];

    // prevents multiple super agents from running at the same time, due to the use of global variables
    let guard = SA_MUTEX.lock().unwrap();

    let cli_cfg = match Cli::init_from(args).unwrap() {
        CliCommand::InitSuperAgent(cli) => cli,
        _ => {
            unimplemented!()
        }
    };

    (cli_cfg, guard)
}
pub fn run_sa(cli_cfg: SuperAgentCliConfig) {
    // Pass the rest of required configs to the actual super agent runner
    SuperAgentRunner::try_from(cli_cfg.run_config)
        .unwrap()
        .run()
        .unwrap();
}
