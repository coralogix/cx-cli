# FORGE-201 — Implementation Plan: guard the Linear state comment against "body too long"

## Goal

Prevent `_write` in `src/saga/services/linear/task_state_store.py` from
crashing when the JSON state comment grows past Linear's GraphQL comment
body limit. Two changes together:

1. **Defensive compaction in `_write`** — measure the formatted body,
   compact when oversized (drop redundant `result` payloads from older
   `step_records`, fall back to a placeholder for `plan_text` referencing
   the plan attachment URL), and — as a secondary safety net — catch a
   Linear "body too long" GraphQL error and retry once with maximum
   compaction.
2. **Stop storing the triage result twice** — the `record_triage` MCP
   tool already appends a `StepRecord(result=…)`; the outcome record
   in `triage/post_step` duplicates it. Halve the per-attempt cost so a
   ticket like FORGE-76 doesn't cross the limit at all in most cases.

Together this satisfies the three success criteria: writes no longer
crash on oversized bodies, `TaskState.plan_text` continues to return
non-None (so the implementation guard passes), and a regression test
covers the oversized-body path.

## Run / check commands

- Lint: `uv run ruff check && uv run ruff format --check && uv run ty check` (or `just lint` when `just` is available).
- Tests (focused): `uv run pytest tests/test_task_state_store.py tests/test_triage.py tests/test_task_state_repo.py -v`.
- Full suite: `uv run pytest`.
- `just` is not installed in this sandbox; commands run directly through `uv`.

## Repro / current behaviour (before the change)

Running the store's `_format_body` against a FORGE-76-shaped
`TaskState` (see `.saga/artifacts/technical_plan.before.txt`) shows:

- 3 triage attempts × 2 records/attempt = 6 records each carrying a
  full `TriageResult`.
- 1 `technical_plan` record whose `TechnicalPlanResult.plan_text`
  is the dominant payload.
- A 50 KB `plan_text` alone pushes the formatted body to ~65 KB,
  which exceeds Linear's `commentUpdate` body limit.

`_write` has no size guard and no branch for a body-too-long error
(only `is_not_modifiable_error` — the credential-rotation fallback).
The failure surfaces as a `LinearHttpError` and the step retries into
the same error, so the tick never recovers.

## Changes

### 1. Detect the error in `src/saga/services/linear/client.py`

Add a helper alongside `is_not_modifiable_error`:

```python
_BODY_TOO_LONG_MARKERS: tuple[str, ...] = (
    "body is too long",
    "value too long",
    "argument validation error",
    "string.max",
)

def is_body_too_long_error(exc: BaseException) -> bool:
    """True when a Linear backend error means the comment body exceeded the size limit.

    Linear flattens GraphQL error extensions to a message string
    (see execute() below), so substring matching is the only signal.
    Conservative marker set to avoid catching unrelated errors.
    """
    msg = str(exc).lower()
    return any(marker in msg for marker in _BODY_TOO_LONG_MARKERS)
```

Nothing else in `client.py` changes.

### 2. Compact oversized state in `src/saga/services/linear/task_state_store.py`

Add a module-level constant and a private compaction helper:

```python
# Safe headroom under Linear's ~65 KB commentUpdate body limit.
_MAX_BODY_CHARS = 60_000

# What we swap plan_text for when a state comment is too big to hold it inline.
# The plan is already durably stored as a Linear attachment (save_plan) and on
# the feature branch (plans/<identifier>.md); the placeholder points readers there.
_PLAN_TEXT_PLACEHOLDER_PREFIX = "<saga:plan-attached "


def _plan_placeholder(asset_url: str | None) -> str:
    if asset_url:
        return f"{_PLAN_TEXT_PLACEHOLDER_PREFIX}url={asset_url}>"
    return f"{_PLAN_TEXT_PLACEHOLDER_PREFIX}url=(none)>"


def _compact_state(state: TaskState, *, aggressive: bool) -> TaskState:
    """Return a copy of ``state`` with non-essential payloads dropped.

    Step 1 (``aggressive=False``) — drop ``result`` from older duplicate
    ``step_records``: for every (step, result-type) pair, keep the result
    only on the latest record and set it to ``None`` on every earlier record.
    This is safe because ``_latest_result`` reads in reverse and only ever
    consults the newest record. It's the primary saving for the FORGE-76
    case (5 redundant TriageResults, ~1 KB each).

    Step 2 (``aggressive=True``) — replace the latest ``technical_plan``
    record's ``TechnicalPlanResult.plan_text`` with a short placeholder
    that references the plan attachment URL from ``state.artifacts``.
    Only applied when a plan artifact exists — otherwise the placeholder
    would erase the only copy of the plan.
    """
    records = [r.model_copy(deep=True) for r in state.step_records]
    seen: set[tuple[str, str]] = set()
    for record in reversed(records):
        if record.result is None:
            continue
        key = (record.step, type(record.result).__name__)
        if key in seen:
            record.result = None
        else:
            seen.add(key)
    if aggressive:
        plan_artifact = state.plan_artifact()
        asset_url = plan_artifact.linear_asset_url if plan_artifact else None
        for record in reversed(records):
            if isinstance(record.result, TechnicalPlanResult):
                record.result = record.result.model_copy(
                    update={"plan_text": _plan_placeholder(asset_url)}
                )
                break
    return state.model_copy(update={"step_records": records})
```

Note: import `TechnicalPlanResult` at the top of the module
(currently absent) — a new `from saga.schemas.technical_plan import
TechnicalPlanResult` line.

Update `_write` to guard on size and catch the error:

```python
async def _write(comment_id: str | None, task_id: str, state: TaskState) -> None:
    body = _format_body(state)
    if len(body) > _MAX_BODY_CHARS:
        logger.warning(
            f"write task={task_id}: body {len(body)} > {_MAX_BODY_CHARS}; "
            "compacting older step_records"
        )
        state = _compact_state(state, aggressive=False)
        body = _format_body(state)
        if len(body) > _MAX_BODY_CHARS:
            logger.warning(
                f"write task={task_id}: still {len(body)} after light compaction; "
                "replacing plan_text with attachment reference"
            )
            state = _compact_state(state, aggressive=True)
            body = _format_body(state)

    new_id: str | None = None
    if comment_id is not None:
        try:
            await _get_client().execute(
                UPDATE_COMMENT, {"id": comment_id, "input": {"body": body}}
            )
            new_id = comment_id
        except LinearHttpError as exc:
            if is_not_modifiable_error(exc):
                # existing behaviour — falls through to CREATE_COMMENT
                logger.warning(
                    f"write task={task_id}: comment {comment_id!r} not modifiable by this actor, "
                    "falling back to CREATE_COMMENT"
                )
                comment_id = None
            elif is_body_too_long_error(exc):
                logger.warning(
                    f"write task={task_id}: Linear reported body too long "
                    f"(len={len(body)}); retrying with aggressive compaction: {exc}"
                )
                state = _compact_state(state, aggressive=True)
                body = _format_body(state)
                await _get_client().execute(
                    UPDATE_COMMENT, {"id": comment_id, "input": {"body": body}}
                )
                new_id = comment_id
            else:
                raise
    if comment_id is None:
        # unchanged create-path — body is already the (possibly compacted) body
        data = await _get_client().execute(
            CREATE_COMMENT, {"input": {"issueId": task_id, "body": body}}
        )
        new_id = ((data.get("commentCreate") or {}).get("comment") or {}).get("id")
    if new_id is not None:
        _cache[task_id] = (new_id, state)
```

Notes:

- `_cache` is populated with the **compacted** `state`, not the
  pre-compaction one — this keeps the read-your-writes invariant
  correct. The cache is authoritative for `ts.plan_text` too, so the
  placeholder (if applied) is what the implementation step reads back.
- Only one retry on "body too long" — if aggressive compaction still
  fails the caller sees the exception (the ticket calls this an
  acceptable outcome; the ticket that hit it is now a genuine outlier).
- The `is_not_modifiable_error` branch is unchanged in behaviour;
  the new branch sits alongside it, and the flow still falls into the
  `if comment_id is None` create-path when appropriate.

### 3. Eliminate duplicate triage storage in `src/saga/orchestrator/steps/triage/__init__.py`

At `triage/post_step` line 240–251, remove `result=result` from the
outcome `StepRecord`:

```python
record = StepRecord(
    step=self.name,
    at=datetime.now(tz=UTC),
    status=out.status,
    session_id=out.session_id,
    summary=result.summary,
    metrics=out.metrics,
    verdict=out.verdict,
    gate=out.gate,
)
```

Also delete the two-line comment explaining why `result=result` was
included; it no longer applies. Rationale:

- `_latest_result` reads records in reverse and returns the newest
  `TriageResult` — the earlier record appended by `record_triage`
  (the MCP tool) or by structured-capture in `step.py` is always
  present by this point (the post_step's own guard at line 152–170
  early-returns if `ts.triage is None`), so this record's `result=`
  is defensive-only and pure duplication.
- `product_definition/post_step` and `technical_plan/post_step`
  already omit `result` from their outcome records — they route
  through `publish_outcome` in `generic.py`, whose StepRecord
  (line 479–488) has no `result=` field. Triage is the odd one out.
- No downstream reader depends on the second copy — `ts.triage`,
  `terminal/aggregate.py` (which never touches `.result`), and
  `failures_for("triage")` (which filters by `failure_class`) all
  keep working.

Effect: FORGE-76-shaped state drops from 6 to 3 `TriageResult`
copies (from 12 records overall to 9), typically enough on its own
to keep the body under `_MAX_BODY_CHARS` for the common case; the
compaction path in `_write` is the safety net for the tail.

### 4. Regression tests in `tests/test_task_state_store.py`

Add three tests below the existing "Cannot modify Comment" section,
reusing the `_LaggyLinear` fake and the `laggy_linear` fixture:

- `test_write_compacts_when_body_exceeds_threshold`: build a
  `TaskState` with several duplicate triage records + a
  `TechnicalPlanResult` big enough to blow past `_MAX_BODY_CHARS`,
  call `update_state`, then verify:
  - The comment body actually posted to `_LaggyLinear` is under
    `_MAX_BODY_CHARS`.
  - Only the latest triage record still has `.result` set (older
    duplicates carry `.result is None`).
  - `_cache["issue-1"][1].plan_text` remains non-None.
- `test_write_retries_on_body_too_long_error`: extend `_LaggyLinear`
  with a `reject_update_body_too_long` toggle that makes the first
  `UpdateComment` raise
  `LinearHttpError("http error: linear graphql errors: Body is too long")`
  and the next succeed. Call `update_state`, then verify the second
  UpdateComment was issued with a body that no longer contains the
  original `plan_text` (i.e. carries the `<saga:plan-attached …>`
  placeholder), and the write eventually succeeded.
- `test_ts_plan_text_survives_compaction`: after aggressive compaction
  (with a plan artifact set in `state.artifacts`), assert
  `state.plan_text` starts with the placeholder prefix, is non-empty,
  and contains the artifact URL.

Also add one small test in `tests/test_triage.py` (or extend an
existing one) asserting that after triage post_step, the outcome
record appended to `step_records` has `record.result is None` — but
`ts.triage` still resolves to the recorded `TriageResult` (from the
earlier MCP-tool record). This locks in the dedup so a future
"defensive" revert can't slip back in.

## Order of changes

Dependencies flow bottom-up:

1. `client.py` — add `is_body_too_long_error` (self-contained).
2. `task_state_store.py` — add `_MAX_BODY_CHARS`, `_compact_state`,
   update `_write`, add the `TechnicalPlanResult` import.
3. `triage/__init__.py` — remove `result=result` from the outcome
   `StepRecord`.
4. Tests — add regression coverage in
   `tests/test_task_state_store.py` and a triage dedup assertion
   in `tests/test_triage.py`.

Do all four in a single change; there's no useful intermediate
state where the tests would pass on only some of them.

## Edge cases and risks

- **`ts.plan_text` returning a placeholder** — implementation
  (`orchestrator/steps/implementation/__init__.py:258–260`) checks
  `if not ts.plan_text`. The placeholder is a non-empty string, so
  the guard passes; the agent gets the placeholder text in its
  kickoff prompt. This is the ticket's explicit trade-off. The
  common case (only the compaction from step 1) never touches
  `plan_text`, so it's the compaction fallback tail.
  Out of scope for this ticket: teaching the implementation step to
  fetch the plan from `plan_artifact()` when it sees the placeholder.
  If the FORGE-76 ticket type actually hits the placeholder path
  in practice, follow that up as a separate ticket.
- **Cache correctness** — `_cache` must hold whatever body Linear
  now has. Storing the compacted state is correct: subsequent reads
  return the compacted state, matching Linear's stored comment.
- **Compaction is safe by construction** — it only drops
  `record.result` values that are duplicates (older than the
  latest same-typed result) and never touches `failure_class`,
  `verdict`, `metrics`, `session_id`, or `summary` — the fields
  every reader consults (see `_derive_step_rows`,
  `_collect_failure_events`, `_pick_verification` in
  `orchestrator/steps/terminal/aggregate.py`).
- **`_migrate_legacy` interaction** — the legacy migration only
  synthesises a `technical_plan` record when no record with a
  result exists. Compaction never removes the latest technical_plan
  record — it may only replace its `plan_text` with a placeholder,
  which is still truthy — so the migration path stays a no-op on
  compacted state.
- **`_MAX_BODY_CHARS` threshold** — 60_000 is conservative. Linear's
  observed limit is around 64–65 KB. If a future Linear tightening
  drops the limit, the size guard catches it; the message-based
  fallback (`is_body_too_long_error`) catches any residual.
- **Threshold marker false positives** — the marker set is
  deliberately narrow (`"body is too long"`, `"value too long"`,
  `"argument validation error"`, `"string.max"`); the pre-write
  size guard is the primary line of defence, so the marker set
  only needs to catch the rare case where the real limit is below
  the guard.
- **`identifier` is not persisted** — `_format_body` already excludes
  it (`exclude={"identifier"}`), so no interaction with the
  compaction path.
- **Concurrency** — writes remain serialised under the per-task
  `asyncio.Lock`; compaction happens inside the lock and does not
  add any awaits.

## Verification

- `uv run pytest tests/test_task_state_store.py -v` — the three
  new tests must pass, plus the four existing ones.
- `uv run pytest tests/test_triage.py -v` — the new dedup
  assertion must pass; the existing triage-post_step tests must
  still pass (in particular the ones that assert `ts.triage` is
  the recorded `TriageResult`).
- `uv run pytest` — full suite green.
- Lint: `uv run ruff check && uv run ruff format --check && uv run ty check`.
- **Manual (out-of-band) sanity check** on a real Linear account is
  not required for this fix — the store's tests use a `LaggyLinear`
  fake at the GraphQL boundary and the change is entirely inside
  that boundary. If a follow-up ticket wants to end-to-end verify
  against a real ticket like FORGE-76, capturing the state comment
  body pre- and post-fix via `saga` against a scratch board is the
  right sequence. Called out here rather than blocking this PR.

## Out of scope for this ticket

- Moving state storage off Linear comments entirely.
- Teaching the implementation kickoff to fetch the plan from
  `plan_artifact()` when it encounters the plan placeholder — the
  ticket calls this out and it can be a follow-up if the FORGE-76
  class of state actually crosses the aggressive-compaction line
  in practice.
- Compacting `ProductDefinitionResult` payloads — small relative
  to `plan_text`, only ever recorded once, and not implicated in
  the FORGE-76 failure.
