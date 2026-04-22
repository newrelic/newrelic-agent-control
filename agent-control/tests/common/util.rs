/// Returns the name of the current test by reading the test thread's name.
///
/// The Rust test harness names each test's thread after its full module path
/// (e.g. `on_host::scenarios::ac_self_update::test_agent_control_self_update_with_oci_registry`).
/// This gives a stable, unique-per-test identifier that is safe to use when
/// concurrent tests need to distinguish their own artifacts from one another.
pub(crate) fn current_test_id() -> String {
    std::thread::current()
        .name()
        .expect("thread name is expected to avoid collisions")
        .to_string()
}
