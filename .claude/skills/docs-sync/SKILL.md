---
name: docs-sync
description: Detects drift between the code and the docs in docs/, fixes it and explains each fix in a markdown report. By default it audits every doc against the whole codebase; `branch` mode only covers what the current branch's changes make false. Use before opening a PR, or when asked whether docs are stale or out of date. Invoke with /docs-sync or /docs-sync branch.
argument-hint: branch audit
---

# docs-sync

Find statements in `docs/` that the code contradicts, fix them, and explain what was done in a report.

## Modes

- `/docs-sync` (default): every doc against the whole codebase. `audit` is an alias.
- `/docs-sync branch`: only statements that `git diff main...HEAD` makes false. Drift the branch did not cause is listed in the report as out of scope and left untouched.

Both modes apply the fixes themselves, without asking.

## Rules

- Report contradictions only: a doc statement that is false. Undocumented behavior is a single short list at the end.
- A finding needs the doc line (`file:line`) and the code evidence (`file:line`), each quoted. Without both, drop it. In the report, write `path:line` on every row: a function name without a line, or "same as above", does not count as evidence. A deleted file or a changelog entry counts as evidence for a removed feature.
- Subagent results are leads, not facts: they have misreported defaults and "missing" code. Verify every finding before it changes a doc.
- Fixes are minimal and state only current behavior. For a stale example, replace only the stale value.
- Edit only `docs/`, plus the report. Never change code, tests or `.changelog/`. Skip `docs/style/` and `docs/RELEASE.md` unless the change touches them.
- If the code looks incomplete or inconsistent (a documented feature that is never wired, dead code, defaults that disagree), leave the doc alone and report it as "Needs decision". Do not delete a documented feature on the strength of a missing call.

## Steps

1. **Scope.**
   - Default: `find docs -name '*.md'`.
   - Branch: `git diff main...HEAD --stat`. If every changed file is a test or under `docs/`, or nothing a doc could observe changed (renames or removals, config keys, flags, env vars, defaults, agent-type schema, behavior, error or log messages), say "no doc impact" and stop without a report. Otherwise grep `docs/` for the changed identifiers and keep the docs that match or whose headings cover the change. Every doc line containing a deleted or renamed identifier is a finding, even if the old value could still work elsewhere.
2. **Extract.** Launch `Explore` subagents in one message: one per doc, or one per batch of small docs (up to ~30 KB). Skip them when one doc and a handful of claims is all there is. Each returns only false claims, as `severity | doc:line | claim | code:line | evidence | fix | confidence (high or low)`, and is told:
   - no evidence, no finding;
   - search the whole repo (`agent-control/src`, `tests`, `test`, `build`, `.github`, `Makefile`, `Tiltfile`) before saying something does not exist, and trace the path that runs in production for defaults (serde, templating, builders);
   - agent-type ids absent from the embedded registry can resolve remotely: list them as "could not verify", not stale, unless a changelog shows they were removed;
   - in branch mode, only check claims that depend on the changes (pass the list).

   Wait for all of them, then merge and dedupe.
3. **Verify** each finding yourself:
   - read the cited code;
   - if the behavior depends on defaults, serde, templating or wiring, or is cheap to run, confirm it with a throwaway test and revert it with `git checkout -- <file>`;
   - if the doc describes something the code lacks, `git log -S'<identifier>'` tells whether it was removed or never existed;
   - for external claims use `gh api` (other repos, specs at the pinned commit) and a status request for each URL, from a script outside the repo; placeholders are not defects.

   A finding that fails goes to "Dropped" with the reason.
4. **Apply** the doc edits.
5. **Report** to `docs-sync-audit.md` at the repo root (overwrite; do not commit):
   - one line of scope: mode, docs covered, docs skipped;
   - per doc, `misleading` rows first, a table `| Line | Sev | Claim | Code evidence | Action |`. Code evidence is `path:line` plus a quote. `Action` is `Applied`, `Applied (adapted)` (say how it differs) or `Needs decision`;
   - then, skipping empty ones: Needs decision (evidence and both options), Dropped (reason), Could not verify, Out of scope (branch mode), Undocumented behavior, Counts, Known limits (behavior claims are best-effort; design rationale is not verified).

   Severity: **misleading** = a reader gets wrong behavior or a failing command; **stale** = outdated examples, names or values; **typo** = wording only.
6. **Check and reply.** `git status --short` shows only `docs/` and the report. Reply with the counts, the "Needs decision" items and the report path; do not paste the report.
