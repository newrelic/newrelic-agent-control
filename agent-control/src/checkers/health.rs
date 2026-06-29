//! Health checking of managed agents and reporting of the results.
/// Publishing of health events.
pub mod events;
/// The core [`health_checker::HealthChecker`] trait, health types, and the checker thread.
pub mod health_checker;
/// No-op health checker that always reports healthy.
pub mod noop;
/// Health value paired with the agent start time.
pub mod with_start_time;

/// Kubernetes health checkers.
pub mod k8s;

/// On-host health checkers.
pub mod on_host;
