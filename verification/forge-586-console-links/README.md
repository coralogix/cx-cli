# PR #176 / FORGE-586 console-link verification suite

`cx` commands print a "View in Coralogix" console link after
create/update/enable/etc. This directory holds the manual-verification
harness for that feature, split into two halves:

- **`automated/`** -- 30 scripts, one per command group, that mechanically
  replay every console-link-eligible subcommand that's safe to re-run with
  zero LLM/human judgment: read-only calls, and mutating calls with an
  already-known-working payload, fresh throwaway names per run, and a
  confirmed delete route. Running these costs nothing but CPU and a live API
  call -- no tokens.
- **`MANUAL_TESTS.md`** (built from `manual/<group>.md`) -- 63 items across
  the 30 groups that genuinely need judgment: unresolved schema-discovery
  black boxes, deciding whether a FAIL is a known pre-existing issue or a
  new regression, anything that would mutate real production data (support
  cases, a real teammate's account) or flip an irreversible setting.

This split came out of a one-time, 740-test manual verification run against
a real Coralogix team (profile `kb-demo`) that exercised every
console-link-eligible subcommand across all three output formats
(text/json/agents). The goal of this directory is to make re-verifying this
feature (after a refactor, a dependency bump, or just before a release)
cheap: run the automated half for free, and only spend LLM time on the
`MANUAL_TESTS.md` items that actually need it.

## Requirements

- A built `cx` binary: `cargo build` from the repo root (debug build is
  fine; `automated/` defaults to `<repo_root>/target/debug/cx`, override
  with `$CX_BIN`).
- Access to a real Coralogix team you're willing to create/delete throwaway
  resources in. Everything here defaults to profile `kb-demo`
  (`$CX_TEST_PROFILE` to override) -- **this is a specific real internal
  team**, not a mock; only run this against a team you have write access to
  and are comfortable with automated create/delete traffic against.

## Running the automated half

```bash
cd verification/forge-586-console-links
python3 run_all_automated.py                 # every group
python3 run_all_automated.py iam alerts      # just these groups
```

Each group script creates its own uniquely-named (uuid-suffixed) throwaway
resources and deletes them in a `finally` block, tolerating "already gone"
on cleanup as non-fatal. A handful of groups are read-only-only (e.g.
`usage`, `schema`) or intentionally skip mutation entirely where no known
delete route exists (see the module docstring at the top of each
`automated/<group>.py` for exactly what it does and does not touch).

Results land in `results/<group>.jsonl` (append-only, one line per
invocation) and `results/<group>.html` (a live-updating fragment,
re-rendered after every call). `run_all_automated.py` finishes by calling
`merge_report.py`, which merges every group's JSONL into a single
`report.html` -- the same report format used for the original PR176
Artifact.

`results/` and `report.html` are regenerated data, not source -- they're
gitignored (see `.gitignore`); the scripts that produce them are what's
committed.

## What's NOT automated, and why

Read `MANUAL_TESTS.md`. Every item has: the exact command last used
(including payload), *why* it can't be safely auto-replayed, decision
criteria for a future pass, and the known baseline (quoted status/notes
from the original run) so you can tell "still the same known issue" from
"something actually changed." A few highlights:

- **`iam scopes create`**, **`iam users create`**, **`alerts
  suppression-rules update`** -- 11-16 schema-discovery attempts each,
  never found a payload shape the backend accepts.
- **`cases`** (update/comment/assign/resolve/close/...) -- no create or
  list of its own; every mutating test target is a real support ticket.
  Resolve/close have no undo.
- **`retentions`** / **`archive`** -- `enableTags` was already flipped
  false->true on `kb-demo` during the original run and can't be reverted
  (backend 501s on update). Never re-run these mutations.
- **`iam api-keys admin set-status`** -- looks like a real, still-present
  backend field-name mismatch bug (confirmed by reading current source, not
  just old test output) -- worth fixing outright rather than re-testing.
- **`iam api-keys list`** -- almost certainly already fixed by commit
  `cc496b5`; flagged for one fresh comparison rather than blind promotion
  to automated, so a regression would actually get noticed.

## Regenerating the Claude Artifact report

The original PR176 verification was published as a Claude-hosted Artifact
for human review:

**https://claude.ai/code/artifact/2fe1fc76-5696-4081-9387-0353928e99dd**

That link is recorded here -- in a committed file, not just chat history --
specifically so any Claude Code session (this one, later, or a fresh chat
with no memory of building this suite) can find and update it just by
reading this repo.

`merge_report.py` writes two files, both gitignored (generated, not
source):

- `report.html` -- a full standalone document (`<!doctype html>` and all).
  Open it directly in a browser for local viewing.
- `artifact_content.html` -- the same content with the `<!doctype>`/
  `<html>`/`<head>`/`<body>` wrapper stripped, because Claude's `Artifact`
  tool wraps whatever file it's given in its own skeleton -- feeding it a
  file that already has its own `<html>`/`<body>` would nest two documents
  inside each other.

To refresh the published Artifact from a Claude Code session:

1. Run the suite (`run_all_automated.py`, plus whatever's needed from
   `MANUAL_TESTS.md`) to regenerate `results/*.jsonl`.
2. Run `python3 merge_report.py` to regenerate both HTML files above.
3. Call the `Artifact` tool with `file_path` pointing at
   `artifact_content.html`, `url` set to the link above (so it updates in
   place instead of minting a new one), and `title` set to
   `"cx CLI — PR #176 command coverage test"` (the content has no `<title>`
   tag of its own on purpose, to avoid the same nesting problem).

Publishing itself still needs an agent turn -- that part of claude.ai isn't
reachable from a plain script -- but everything upstream of it (steps 1-2)
is now free.

## Layout

```
harness.py              shared run_cx()/record() helpers used by every automated/ script
merge_report.py          merges results/*.jsonl into report.html
run_all_automated.py     runs every automated/<group>.py and regenerates the report
automated/<group>.py     30 scripts -- one per cx command group
manual/<group>.md        30 docs -- source fragments concatenated into MANUAL_TESTS.md
MANUAL_TESTS.md           the concatenated, judgment-required backlog
payloads/*.json          known-working request bodies the automated scripts render from
```
