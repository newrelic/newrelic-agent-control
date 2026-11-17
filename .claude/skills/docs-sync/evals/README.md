# docs-sync evals

Regression tests for the skill. Each case replays a real commit from this repo's history as a branch whose author forgot the docs, so the commit's own doc edits are the ground truth.

| Case | Commit | Expected |
|---|---|---|
| shared-fs-cleanup | `6279d31d` | Flags the stale "cleanup not implemented" note |
| log-filenames | `dc530c24` | Flags the dated log names in both file-tree diagrams |
| delete-ohi-0-1-0 | `562a0bca` | Flags the `nri_redis:0.1.0` examples |
| log-retention | `8e326732` | No contradictions |
| refactor-windows-crates | `c0ff4409` | No findings |
| refactor-namespaced-var | `8a76244c` | No findings |

These cases run `branch` mode. The default mode (full audit) has no stable ground truth because the docs keep changing; check it by hand.

## Run

1. Build the fixtures (needs the commits in local history):
   ```bash
   W=/tmp/docs-sync-workspace/iteration-N
   for c in $(python3 -c "import json;[print(e['name']+':'+e['commit']) for e in json.load(open('.claude/skills/docs-sync/evals/evals.json'))['evals']]"); do
     .claude/skills/docs-sync/evals/build_fixture.sh "${c#*:}" "$W/fixtures/${c%%:*}"
   done
   ```
2. For each case, launch one subagent (all in one message). Prompt:
   > Execute `/docs-sync branch`. Read `.claude/skills/docs-sync/SKILL.md` and follow it exactly. Working repo: `$W/fixtures/<name>` (a throwaway copy; base `main`, branch `feature`), so applying doc edits there is fine. Do not look at any other repo. When the skill writes `docs-sync-audit.md`, copy it to `$W/<name>/with_skill/outputs/report.md`; when it says "no doc impact", save that message there instead. Afterwards save `git diff` to `outputs/docs.diff` and `git status --short` to `outputs/status.txt`, both from the fixture.
3. Save each run's `total_tokens` from the completion notice to `$W/<name>/with_skill/timing.json` as `{"total_tokens": N}`.
4. Grade:
   ```bash
   python3 .claude/skills/docs-sync/evals/grade.py $W
   ```

## When to run

After any edit to `SKILL.md`. Compare against the previous iteration's pass counts and token totals. Add a case whenever a real drift slips past the skill or the skill flags something that was not drift.

## Last known baseline

6 of 6 cases pass on the 53-line `SKILL.md`: the four cases with a finding or an expected non-finding ran on the version before the evidence rule moved into Rules, and the two that had failed the evidence check (`shared-fs-cleanup`, `log-filenames`) were re-run on the final wording and pass 8/8. Per-run token counts were not captured. The previous 121-line skill passed 4 of 6 (it edited unrelated lines in `log-retention` and left evidence without line numbers).
