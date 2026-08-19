use crate::common::docker_hub::latest_published_ac_tag;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::service::{
    ENABLED, STATUS_RUNNING, get_service_status, get_unit_file_state, restart_service_and_wait,
};
use std::time::Duration;
use tracing::info;

/// Regression test: a package-manager upgrade must leave the service enabled and running.
/// Installs the version under test first since its preremove is what runs during the upgrade.
pub fn test_package_manager_upgrade_keeps_service_enabled(args: InstallationArgs) {
    let version_under_test = args.agent_control_version.clone();
    let upgrade_to_version = retry_panic(
        10,
        Duration::from_secs(2),
        "fetching latest AC tag from Docker Hub",
        latest_published_ac_tag,
    );
    assert_ne!(
        version_under_test, upgrade_to_version,
        "version under test and latest published version must differ for this test to be meaningful."
    );

    let _clean_up = CleanUp::new(tear_down_test);

    info!(
        version = version_under_test,
        "Installing the version under test"
    );
    install_agent_control_from_recipe(&RecipeData {
        args: args.clone(),
        ..Default::default()
    });

    restart_service_and_wait(linux::SERVICE_NAME, STATUS_RUNNING);
    assert_eq!(
        get_unit_file_state(linux::SERVICE_NAME),
        ENABLED,
        "service should be enabled right after a fresh install"
    );

    info!(
        version = upgrade_to_version,
        "Upgrading Agent Control via the package manager"
    );
    let mut upgrade_args = args;
    upgrade_args.agent_control_version = upgrade_to_version;
    // Fetch the real release from the production repo, not the local build under test.
    upgrade_args.artifacts_package_dir = None;
    install_agent_control_from_recipe(&RecipeData {
        args: upgrade_args,
        ..Default::default()
    });

    info!("Verifying the service is still enabled and running after the upgrade");
    assert_eq!(
        get_unit_file_state(linux::SERVICE_NAME),
        ENABLED,
        "service must remain enabled after a package-manager upgrade \
         (regression: the old package's preremove ran during the upgrade and disabled it)"
    );
    assert_eq!(
        get_service_status(linux::SERVICE_NAME),
        STATUS_RUNNING,
        "service should still be running after the upgrade"
    );
}
