# `generate_docs_release_notes` — docs-website release-notes generator

`run.sh` generates the Agent Control release-notes `.mdx` for
[newrelic/docs-website](https://github.com/newrelic/docs-website) from
`CHANGELOG.md`.

## Run generator

The generator is a binary that given the changelog file and version, will generate
a `.mdx` file.

```bash
./generate_docs_release_notes/run.sh CHANGELOG.md 1.19.0
```

## Execute the test runner

The test runner is a binary that can run in isolation. No inputs are required.

The folder from where it's executed doesn't matter.

```bash
# From the root of the project
./tools/generate_docs_release_notes/test.sh

# From the tools folder
./generate_docs_release_notes/test.sh

# From outside the project folder
./newrelic-super-agent/tools/generate_docs_release_notes/test.sh
```

## Test data

| Input changelog | Golden output | Exercises |
|---|---|---|
| `changelog_full.md` | `expected_full.mdx` | all sections populated, PR/commit-ref stripping, extract only the target version |
| `changelog_deps_only.md` | `expected_deps_only.mdx` | only Dependencies → all three arrays omitted |

The `changelog_*.md` files intentionally include an older version section below
the target to prove the date/extraction picks only the requested `<VERSION>`.
