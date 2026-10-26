# Release process

This document describes how a release is cut for Agent Control, how `CHANGELOG.md` drives it, and how the next version is calculated automatically.

## Overview

Releases are cut in three phases:

1. **Record changes** — every PR with a user-visible change adds an entry to the `CHANGELOG.md`.
2. **Open a release PR** — a maintainer triggers a workflow that computes the next version, rewrites `CHANGELOG.md`, bumps `Cargo.toml`, and opens a PR.
3. **Publish** — after the release PR is merged, a GitHub pre-release needs to be created which triggers the pre-release workflows. Promoting the pre-release will publish the artifacts to production.

## 1. Record changes — `CHANGELOG.md`

`CHANGELOG.md` follows the [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) structure with sections understood by the [release-toolkit](https://github.com/newrelic/release-toolkit):

- `## Unreleased` — pending changes, grouped by type:
  - `### Enhancements` — additions / improvements
  - `### Bug fixes`
  - `### Breaking changes` — (this will cause a major bump)
  - `### Security notices`
  - `### Dependencies` — auto-populated from `renovate` / `dependabot` commits by the toolkit; you don't normally write these by hand

Every PR must add a change file (see below), which is validated on the CI. The check is skipped when the PR has the `dependencies` or `skip-changelog` label, or when the head branch starts with `renovate/` or `generate-release/`, the toolkit will pick the dependency changes up automatically at release time.

### Change files `.changelog/`

Instead of editing `## Unreleased` (which makes PRs conflict with each other), a PR adds one small YAML file to `.changelog/`. Copy [`.changelog/TEMPLATE.yaml`](../.changelog/TEMPLATE.yaml) to `.changelog/<branch-or-pr>.yaml` and fill it in:

```yaml
type: bugfix   # breaking | security | enhancement | bugfix | dependency
message: Description of the change in a single line.
```

The `type` sets the version bump (see [Automatic version calculation](#automatic-version-calculation)). Only add a change file if the change affects what a user of Agent Control experiences: a new capability, a changed behavior, or a fixed bug. Skip:
- Internal refactors with no behavior change
- Documentation-only changes
- Test-only changes
- Pipeline/CI changes, unless they add support for something new (e.g. a new build target)

The toolkit gathers the change files (and any entry already in `## Unreleased`). The CI validates the files, and the release PR workflow moves them into `CHANGELOG.md` and deletes them, keeping the template.

To preview the `CHANGELOG.md` that the release-toolkit would generate, run `make rt-update-changelog` locally — it invokes the same toolkit script used by the release workflow and rewrites the file in place so you can verify the output before opening a release PR.

## 2. Open a release PR

To cut a release, run [`on_demand_generate_prerelease_pr.yml`](../.github/workflows/on_demand_generate_prerelease_pr.yml) manually (Actions → **Generate pre-release PR** → **Run workflow**, from the default branch). This will:

1. Runs `release-toolkit` against `CHANGELOG.md`. If `## Unreleased` has no entries and no dependency commits, the workflow **fails loudly** — a manual dispatch with nothing to release is treated as a misconfiguration.
2. Rewrites `CHANGELOG.md` moving the contents of `## Unreleased` and of the change files in `.changelog/`, plus all detected dependency bumps from the commits since the last tag, into a new version. The released change files are deleted in the release commit.
2. Computes the next version (see [Automatic version calculation](#automatic-version-calculation)).
4. Bumps `[package].version` in `agent-control/Cargo.toml`.
5. Creates a branch `generate-release/<version>` (fails if it already exists).
6. Commits `CHANGELOG.md` + `Cargo.toml` via the GraphQL `createCommitOnBranch` mutation so the commit is **signed automatically** on behalf of the `agent-control-app` GitHub App (`main` has a branch rule requiring signed commits).
7. Opens a pull request against `main` with the `release` label.

After review, merging the PR produces the release commit on `main`.

## 3. Publish

Publishing is currently a **manual** step. Once the release PR is merged, open the latest version entry in `CHANGELOG.md` (the `## <version> - <YYYY-MM-DD>` section just added) and use:

- **Tag name / title** → `<version>` from that heading
- **Release notes body** → the contents of that section

Then create a GitHub **pre-release** which will fire the pre release pipeline. 

## Automatic version calculation

The next version is computed by [release-toolkit's `next-version`](https://github.com/newrelic/release-toolkit#next-version):

1. The toolkit reads the change files in `.changelog/` and any `## Unreleased` entries from `CHANGELOG.md`, plus any `renovate` / `dependabot` commits since the last tag, and produces a transient `changelog.yaml`.
2. The **current version** is taken from the highest existing git tag. This repository uses tags **without** a `v` prefix (`1.14.0`, not `v1.14.0`); that's controlled by `output-prefix: ""` passed to the toolkit.
3. The next version bumps the current one according to the **highest-impact** change type present in `changelog.yaml`:

   | Change type   | Semver bump |
   |---------------|-------------|
   | breaking      | major       |
   | security      | minor       |
   | enhancement   | minor       |
   | bugfix        | patch       |
   | dependencies  | patch       |

4. The same version is written into `[package].version` in `agent-control/Cargo.toml`.

Because the version is derived from the changelog, **the changelog is the source of truth** for release scope — getting it right in the PR is what determines both the release notes and the version number.
