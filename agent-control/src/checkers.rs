//! Periodic checkers that observe managed agents and report derived signals.
//!
//! Contains the [`guid`] (entity GUID), [`health`], and [`version`] checkers, each with
//! Kubernetes and/or on-host implementations.
pub mod guid;
pub mod health;
pub mod version;
