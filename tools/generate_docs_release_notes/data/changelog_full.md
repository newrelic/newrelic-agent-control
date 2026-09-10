# Changelog

All notable changes are documented in this file.

## Unreleased

## v1.99.0 - 2026-07-15

### 🚀 Enhancements
- Add support for shared filesystems in on-host agent types (#1234)
- On-host self-update now skips sub-agent reconciliation while an update is in progress

### 🐞 Bug fixes
- Restore `local_config.yaml` from the `.rpmsave` backup left by a prior uninstall (#1240)
- Fix a mis-recording of the Instrumented metric (a1b2c3d)

### 🛡️ Security notices
- Bump base image to patch CVE-2026-0001

### ⛓️ Dependencies
- Updated rust crate chrono to 0.4.45
- Updated alpine/helm to v4.2.1

## v1.98.0 - 2026-07-01

### 🚀 Enhancements
- A previous release enhancement that must NOT appear in the 1.99.0 notes
