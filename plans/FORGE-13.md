
# FORGE-13 — Saga Terminal Summary: Implementation Plan

## Goal

Enhance `aggregate_terminal()` so the one-time Terminal Summary it posts to Linear (and mirrors to Slack) is a complete retrospective of the task: aggregated cost/time/tokens, per-step narrative with friction breakdown, triage retrospective, PR impact, verification verdict, artifacts list, and a goal/AC lookback. The idempotency guard (`aggregated_at`) and the trigger sites (`pr_monitor._finalize_merged` for merge → Staging, `loop.py` cancel path for → Canceled) are out of scope.

## How to run & verify

Repo: `saga/`, Python 3.13 + `uv`.

- **Tests:** `just test` (full suite); `just test tests/test_terminal_aggregate.py` while iterating.
- **Lint/types:** `just lint-fix && just lint`.
- **No live Linear needed:** the tests use the `task_states` fixture (in-memory state store).
- **Live exercise of the rendered comment** is not feasible from this sandbox — instead we render the markdown by exercising `aggregate_terminal()` against a representative `TaskState` and inspecting the captured Linear comment string. The "before" snapshot is at `.saga/artifacts/before_summary.md`; the corresponding "after" snapshot will be written there once the implementation lands.

## Current behaviour (observed, before any change)

Running `aggregate_terminal` against a representative TaskState (triage tripwire-fired, technical_plan with one agent_error then DONE, implementation with one verify_reject then PASS, pr_review CI friction, three uploaded artifacts, one archived run) produces:

```
📋 **Terminal Summary — In Staging**

**Triage**: feature · risk=high · complexity=high

**Steps**:
- triage: done (0 attempt(s))
- technical_plan: done (1 attempt(s))
- implementation: done (1 attempt(s))
- pr_review: — (1 attempt(s))

**PR**: coralogix/saga#42

[Plan](https://assets.example.com/plan.md)

**Prior attempts**: 1
```

Gaps (vs FORGE-13 success criteria):

1. No tokens / cost / duration aggregation — the `TerminalSummary.total_tokens`/`total_seconds` fields exist but are never populated, and there is no cost field at all.
2. Per-step rows expose only status and a count; the `summary`, `metrics`, `verdict`, and the *kind* of friction (agent_error vs verify_reject vs ci_failure vs merge_conflict vs needs_input) are not surfaced.
3. `pr_review` shows "—" because friction-only records carry no `status`; the row collapser loses friction-only steps.
4. Triage tripwires, repos, and the narrative `summary` are not rendered.
5. Verdict/certainty/notes on the latest successful records are dropped.
6. Artifacts other than the plan are hidden.
7. There is no "goal / AC met?" lookback.

## Data already on `TaskState` (no schema additions needed)

Everything required is on existing records — confirmed by reading `src/saga/schemas/state.py`, `src/saga/schemas/step.py`, and the call sites that write the data:

- `step_records[*].metrics` → `total_tokens`, `total_cost_usd`, `duration_ms` per turn (written in `publish_outcome`).
- `step_records[*].failure_class` (`FailureClass`) → the friction taxonomy already exists (`AGENT_ERROR`, `VERIFY_REJECT`, `CI_FAILURE`, `MERGE_CONFLICT`, `REPLAN`, `NEEDS_INPUT`).
- `step_records[*].verdict` → `Verdict(result, certainty, notes)` — written by `record_verifier_fail` and by `publish_outcome` on success.
- `step_records[*].summary` → human-facing summary captured on the outcome record.
- `step_records[*].result` → `TriageResult` / `ProductDefinitionResult` / `TechnicalPlanResult` — already reachable via `ts.triage`, `ts.product_definition`, `ts.technical_plan`, `ts.plan_text`.
- `ts.artifacts` → all `StepArtifact`s (each carries `step`, `kind`, `caption`, `linear_asset_url`, `pr_comment_url`, optional `phase`).
- `ts.prs`, `ts.runs`, `ts.consecutive_failures`, `ts.branch_name`, `ts.identifier` — already accessible.

## What needs to change

Two files implement the change; one test file gets new assertions. No new fields on `TaskState` (per "Out of Scope"), and the idempotency guard stays put.

### 1) `src/saga/schemas/state.py` — extend the transient summary types

`TerminalSummary` is built transiently inside `aggregate_terminal()` (not persisted), so we can grow its shape freely. `TerminalStepRow` likewise.

- **`TerminalStepRow`** — add the fields needed for the per-step rendering:
  - `friction: dict[str, int] = {}` — counts of `failure_class.value` for the step (e.g. `{"agent_error": 1, "verify_reject": 1}`).
  - `latest_verdict: Verdict | None = None` — verdict on the last successful record for the step (used for the verification section).
  - `metrics_total: StepMetrics | None = None` — per-step rollup of `total_tokens` / `total_cost_usd` / `duration_ms` (built by summing turn metrics).
  - Keep `name`, `status`, `attempts`, `summary` as today. `attempts` keeps its current meaning (= friction count); we no longer rely on it to communicate "the step succeeded after N retries" — the row's `status` + `friction` do that better.
- **`TerminalSummary`** — add:
  - `total_cost_usd: float | None = None` (new — the ticket calls it out).
  - `total_duration_seconds: float | None = None` (new — keep the existing `total_seconds` as an alias-by-rename: rename `total_seconds` → `total_duration_seconds` and update the one reader in `aggregate.py`; the field has no callers outside this module, confirmed via grep).
  - `triage_summary: str | None = None` — the narrative from `TriageResult.summary`.
  - `triage_tripwires: list[str] = []` and `triage_repos: list[str] = []` — surface the bits not currently rendered.
  - `acceptance_criteria: list[str] = []` and `non_goals: list[str] = []` — copied from `ts.product_definition` when present (used for the lookback section).
  - `plan_text: str | None = None` — copied from `ts.plan_text`; used for the lookback "did we ship against the plan?" check.
  - `plan_no_changes_needed: bool = False` and `plan_no_changes_reason: str = ""` — surface the "no code change required" terminal path the technical_plan step can take.
  - `artifacts: list[StepArtifact] = []` — the full list (currently only `plan_asset_url` is exposed). Keep `plan_asset_url` for back-compat with the existing tests.
  - `failure_events: list[StepRecord] = []` — every record with `failure_class is not None`, newest last. Used to render the friction-analysis section.
  - `verifier_verdicts: list[tuple[str, Verdict]] = []` — `(step_name, verdict)` for the latest successful verdict per step that had one. Used for the verification section.

  Keep `narrative` as a future-use field (currently unused; do not remove — out of scope to clean up).

These are all additive (defaults preserve back-compat with serialised pre-FORGE-13 payloads — though `TerminalSummary` is not persisted, so this is mostly hygiene).

### 2) `src/saga/orchestrator/steps/terminal/aggregate.py` — aggregation + rendering

#### 2a) Replace `_derive_step_rows` with a richer collapse

Today it iterates `step_records` once, keying by `record.step`, bumping `attempts` when `failure_class is not None` and overwriting `status`/`summary`. Replacement:

```python
def _derive_step_rows(ts: TaskState) -> list[TerminalStepRow]:
    rows: dict[str, TerminalStepRow] = {}
    for record in ts.step_records:
        row = rows.setdefault(record.step, TerminalStepRow(name=record.step))
        if record.failure_class is not None:
            row.attempts += 1
            cls = record.failure_class.value
            row.friction[cls] = row.friction.get(cls, 0) + 1
        if record.status is not None:
            row.status = record.status.value
        if record.summary:
            row.summary = record.summary
        if record.metrics is not None:
            row.metrics_total = _add_metrics(row.metrics_total, record.metrics)
        if record.verdict is not None and record.status is not None:
            row.latest_verdict = record.verdict
    return list(rows.values())
```

`_add_metrics(a, b)` is a small helper inside this module that returns a `StepMetrics` summing `total_tokens`, `total_cost_usd`, and `duration_ms` (None + x → x). Other fields on `StepMetrics` are turn-specific and not summed.

#### 2b) Aggregated totals

A second pure helper sums across **all** `step_records[*].metrics` (one source of truth, includes friction records that happen to carry metrics):

```python
def _aggregate_totals(ts: TaskState) -> tuple[int | None, float | None, float | None]:
    """Return (total_tokens, total_cost_usd, total_duration_seconds), each None if no record had it."""
```

Skip None values so a single missing field doesn't zero the total.

#### 2c) Friction events list

```python
def _failure_events(ts: TaskState) -> list[StepRecord]:
    return [r for r in ts.step_records if r.failure_class is not None]
```

#### 2d) Verifier verdicts (latest per step that had one)

```python
def _verifier_verdicts(ts: TaskState) -> list[tuple[str, Verdict]]:
    latest: dict[str, Verdict] = {}
    for r in ts.step_records:
        if r.status is not None and r.verdict is not None:
            latest[r.step] = r.verdict
    return list(latest.items())
```

#### 2e) Rebuild the `TerminalSummary` construction

Inside `aggregate_terminal()`:

```python
totals = _aggregate_totals(ts)
plan = ts.technical_plan
pd = ts.product_definition
plan_artifact = ts.plan_artifact()

summary = TerminalSummary(
    terminal_status=terminal_status,
    triage=ts.triage,
    triage_summary=ts.triage.summary if ts.triage else None,
    triage_tripwires=list(ts.triage.tripwires) if ts.triage else [],
    triage_repos=list(ts.triage.repos) if ts.triage else [],
    steps=_derive_step_rows(ts),
    runs=ts.runs,
    prs=ts.prs,
    plan_asset_url=plan_artifact.linear_asset_url if plan_artifact else None,
    plan_text=ts.plan_text,
    plan_no_changes_needed=plan.no_changes_needed if plan else False,
    plan_no_changes_reason=plan.no_changes_reason if plan else "",
    acceptance_criteria=list(pd.acceptance_criteria) if pd else [],
    non_goals=list(pd.non_goals) if pd else [],
    artifacts=list(ts.artifacts),
    failure_events=_failure_events(ts),
    verifier_verdicts=_verifier_verdicts(ts),
    total_tokens=totals[0],
    total_cost_usd=totals[1],
    total_duration_seconds=totals[2],
)
```

#### 2f) Rewrite `_summary_comment(summary)` — eight sections, gracefully degrade

Each section is built by a tiny helper that returns either a list of lines or an empty list, and the top-level composer concatenates non-empty sections with a single blank line between them. Sections in fixed order, matching the ticket's success criteria:

1. **Header** — `# 📋 Terminal Summary — {status}` (markdown H1; renders nicely in both Linear and via `to_mrkdwn`). Append `({task.identifier})` only if `ts.identifier` is set (read on `aggregate_terminal` entry, passed via the summary).
2. **Run totals** — single bullet line: `**Totals:** {tokens} tokens · ${cost:.2f} · {duration:.0f}s`. Omitted entirely when all three are None.
3. **Triage retrospective** — `**Triage:** {ticket_type} · risk={risk} · complexity={complexity}`, plus indented sub-lines for `Repos: …`, `Tripwires: …` (only if non-empty), and `Summary: …`. Render only when `summary.triage is not None`.
4. **Acceptance criteria & non-goals** — bulleted list of ACs (omit section when empty); ditto non-goals. Optional `> Plan: <link>` line under the header when `plan_asset_url` is set.
5. **Steps** — for each row, render:
   - `- **{name}**: {status or '—'} · {attempts} friction event(s)` (singularised when count == 1)
   - on the next indented line, the friction breakdown when non-empty (e.g. `friction: agent_error×1, verify_reject×1`)
   - on the next indented line, the row's `summary` when present
   - on the next indented line, per-step metrics rollup (`{tokens} tok · ${cost:.2f} · {dur:.0f}s`) when any present.
   This replaces the current minimal one-liner per step.
6. **Failure & friction breakdown** — when `failure_events` is non-empty: bulleted log of `- {step} attempt {n}: {failure_class.value} — {summary or '(no detail)'}`. Cap at 10 most recent (newest last); add `… and {N} earlier` when truncated, so the comment stays readable on tickets with replay loops.
7. **Verification** — when `verifier_verdicts` is non-empty: one line per `(step, verdict)`: `- **{step}**: {result.value}{ certainty hint}{ — notes}`. The certainty hint is `(certainty=0.95)` when present and otherwise omitted. Mirrors the format already used by `_outcome_comment` so reviewers see consistent phrasing.
8. **PRs & runs** — `**PR(s):** repo#N, repo#M` (existing wording), then `**Branch:** {branch}` when `ts.branch_name` is set, then a one-line CI/conflict tally summed from `PRState.ci_attempts`/`conflict_attempts` when non-zero (e.g. `CI retries: 2 · Merge-conflict retries: 1`). Archived runs become `**Prior attempts:** N` (only when ≥ 1) with a sub-bullet per archived run linking its `pr_number` and `plan_asset_url` when set.
9. **Artifacts** — bulleted list of `[{caption}]({linear_asset_url})`, prefer `linear_asset_url`, fall back to `pr_comment_url`, skip an artifact when both are absent. Group by `kind` only if more than 3 items, otherwise flat.
10. **Lookback** — short closing paragraph:
    - If `terminal_status` is the staging name: "Shipped via {pr refs} — {N} acceptance criterion/criteria recorded, {M} captured artifact(s)."
    - If terminal_status is Canceled / Pause.STOPPED-implied: "Canceled — work did not reach staging." Include the final pause reason if a `NEEDS_INPUT` failure record exists (use `summary`).
    - If `plan_no_changes_needed`: "No code change required — {plan_no_changes_reason}."

(The numbering above maps 1→header, 2→aggregated metrics, 3→triage retrospective, 4→AC/plan, 5→per-step breakdown, 6→failure analysis, 7→verification status, 8→PR impact, 9→artifacts, 10→task lookback. Sections 4 and 8 cover the ticket's two intertwined points (4 = "task lookback / acceptance criteria"; 8 = "PR impact summary"); they're rendered as separate sections so absent data doesn't drag in unrelated wording.)

Use a single composer (e.g. `_compose(*section_lines_lists)`) that joins non-empty sections with `"\n\n"` so absent sections simply disappear — the existing graceful-degradation behaviour.

Keep the Slack mirror flow exactly as today: `to_mrkdwn(body)` posts the same content. Verify by running an empty-state TaskState through to_mrkdwn — should still yield a non-broken message.

#### 2g) Pull `ts.identifier` into the header

`aggregate_terminal()` already has `task: LinearTask`; the human-readable id is `task.identifier`. Pass it explicitly into the body builder so the rendered comment leads with `FORGE-13` etc.

### 3) `tests/test_terminal_aggregate.py` — new assertions

Keep all existing tests (they still pass under the new structure because the new sections are additive and degrade to empty). Add the following new tests, all using the `task_states` fixture and `AsyncMock` tracker/notifier (same shape as existing tests):

1. **`test_aggregate_renders_totals`** — `TaskState` with three records carrying `metrics(total_tokens, total_cost_usd, duration_ms)` (e.g. 100+200+500 tokens, $0.01+$0.02+$0.04, 1000+2000+3000 ms). Assert the comment body contains `"800 tokens"`, `"$0.07"`, and `"6s"` (or whatever the chosen formats are — the test pins the exact rendered output).
2. **`test_aggregate_renders_per_step_friction`** — TaskState with a step that has two `AGENT_ERROR` records followed by a `DONE` record. Assert the comment body contains the step name, `done`, and `agent_error×2`.
3. **`test_aggregate_renders_friction_section`** — TaskState with mixed `agent_error`, `verify_reject`, `ci_failure`. Assert all three appear in a "Failure & friction" / friction breakdown section with the right step attribution.
4. **`test_aggregate_renders_verification_verdict`** — TaskState with a `DONE` record whose `verdict.result == PASS` and notes `"diff verified"`. Assert the body contains `"pass"` and the notes string.
5. **`test_aggregate_renders_triage_retrospective`** — TaskState whose `TriageResult` has `tripwires=["security"]` and `repos=["internal-saga"]`. Assert both strings appear in the comment.
6. **`test_aggregate_renders_acceptance_criteria_when_present`** — TaskState with a `product_definition` record carrying ACs. Assert each AC line appears.
7. **`test_aggregate_renders_all_artifacts`** — TaskState with three artifacts (plan + before screenshot + after screenshot). Assert all three captions and their URLs are in the body.
8. **`test_aggregate_renders_pr_impact_retry_counters`** — TaskState whose `PRState` has `ci_attempts=2` / `conflict_attempts=1`. Assert the rendered text contains `"CI retries: 2"` and `"Merge-conflict retries: 1"`.
9. **`test_aggregate_lookback_shipped`** — TaskState in staging terminal status with PRs and ACs. Assert a "Shipped via …" sentence is present.
10. **`test_aggregate_lookback_canceled`** — TaskState with `terminal_status="Canceled"`, no PRs. Assert the body contains a "Canceled — work did not reach staging." line.

Also add a small `test_aggregate_totals_skip_none` — three records, one with `total_cost_usd=None`. Assert the rendered cost equals the sum of the two non-None values (and a None field never produces `"$None"` in the output).

The existing `test_aggregate_graceful_only_triage` and `test_aggregate_graceful_empty_state` continue to assert the comment is produced without raising — the new code paths must not raise on missing data (they only render sections whose source is present).

### 4) Update the "after" artifact

After the implementation lands, capture the rendered comment again (same representative `TaskState` as in `.saga/artifacts/before_summary.md`) and write to `.saga/artifacts/after_summary.md` so reviewers can diff before/after.

## Order of changes (dependencies first)

1. Extend `TerminalStepRow` and `TerminalSummary` in `src/saga/schemas/state.py` (additive fields, all defaulted). Rename `total_seconds` → `total_duration_seconds` and update the single reader in `aggregate.py` in the same change.
2. Add the pure helpers in `aggregate.py`: `_add_metrics`, `_aggregate_totals`, `_failure_events`, `_verifier_verdicts`. Replace `_derive_step_rows` with the richer version.
3. Rewrite `_summary_comment(summary)` to render the eight sections, with one helper per section returning a list of lines.
4. Update `aggregate_terminal()` to populate the new fields when building `TerminalSummary`. Pass `task.identifier` through so it can land in the header.
5. Add the new tests in `tests/test_terminal_aggregate.py`. Run `just test tests/test_terminal_aggregate.py` while iterating.
6. Run `just lint-fix && just lint && just test` end-to-end.
7. Re-render the example TaskState and write `.saga/artifacts/after_summary.md`.

Stages 1 and 2 are pure data, stage 3 is pure formatting; doing them in this order keeps each diff small and unit-testable.

## Edge cases & risks

- **Empty TaskState** — every helper must short-circuit on missing data; the existing `test_aggregate_graceful_empty_state` test pins this. The composer skips empty sections, so an empty state still produces a minimal-but-valid comment.
- **Mixed None / present metrics** — sum helpers must skip None per field, not per record (a record might have `total_tokens` but no `total_cost_usd`).
- **pr_review friction-only steps** — today the row's `status` ends up `None` because no DONE record exists for pr_review. The new format renders `pr_review: — · 1 friction event (ci_failure×1)`, which is correct — no need to fake a status.
- **Step name not yet seen** — `_derive_step_rows` already creates the row on first record; the additions preserve this. No change to ordering (first-seen).
- **Slack mrkdwn conversion** — `to_mrkdwn` handles `**bold**`, `#` headers, and `[label](url)`. The new sections use only these markdown features; no new conversion rules needed. Tables would need code fences; we render bullets, not tables.
- **Comment length** — Linear comments can be long, but pr_review tickets with many friction events could blow up. The friction-events cap (10, with "… and N earlier") and the artifacts grouping-by-kind threshold (>3) bound the worst case.
- **Idempotency** — unchanged; `aggregated_at` still gated on a successful Linear post, Slack still best-effort. The existing `test_aggregate_idempotent_*` tests continue to pin this.
- **Cancel path clearing step_records** — `loop.py` clears non-failure records *after* `aggregate_terminal` runs, so aggregation sees the full record set. Verified by grep: `aggregate_terminal` is called at line 455, `step_records=[r for r in ts.step_records if r.failure_class is None]` at line 492.
- **Out-of-scope reminders** — no new `TaskState` fields, no change to the aggregation trigger, no agent turn. Pure deterministic rendering.

## Verification

After implementing, all three must be true:

1. `just lint` clean.
2. `just test` clean — both the original 8 tests in `test_terminal_aggregate.py` and the ~11 new ones.
3. Re-running the snippet that produced `.saga/artifacts/before_summary.md` against the same TaskState yields a comment that contains, in order: `Terminal Summary — In Staging`, the `Totals:` line, a `Triage` block with `Tripwires:`, the AC list (when present), per-step rows with `done`/`friction:` fragments, a `Failure & friction` section listing the agent_error / verify_reject / ci_failure events, a `Verification` section with the implementation PASS verdict, `coralogix/saga#42`, all three artifact captions, and a "Shipped via" lookback line. Saved to `.saga/artifacts/after_summary.md`.

Both `before_summary.md` and `after_summary.md` will be uploaded automatically by the saga artifact pipeline once the step completes.
