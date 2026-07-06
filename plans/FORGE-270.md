# FORGE-270 — Fix stale memory source inserts for missing memories in bulk memory updates

## Goal

Prevent `_bulk_add_sources` from ever attempting to insert a `team_memory_sources` row for a `memory_id` that no longer exists in `team_user_memories`, and stop `memory_extraction.py` from mis-labeling every FK violation as "chat was deleted".

Two failure paths must be closed:

1. **External race** — a memory read at step "read_memories" is deleted (directly, or via the `delete_orphaned_memories` trigger cascading from a chat delete) before "update_memories" runs. `_bulk_edit` finds it missing and only logs, but `_bulk_add_sources` still tries to insert a source row for that id → `memory_sources_memory_id_fkey` violation.
2. **In-batch conflict** — the same operation batch contains a `DeleteMemoryOperation` and an `EditMemoryOperation` for the same `memory_id`. `_bulk_delete` runs first and removes the row, then `_bulk_edit` finds it missing, but the queued source insert still fires.

## Baseline / current behavior

Reviewed `apps/api/src/api/repositories/memory_repository.py`:

- `bulk_apply_memory_operations` (lines 203–234) drives the sequence `_bulk_delete → _bulk_edit → _bulk_insert → _bulk_add_sources` inside one transaction.
- `_split_bulk_operations` (lines 237–259) appends `MemorySource(op.memory_id, op.chat_id)` for **every** `EditMemoryOperation`, unconditionally, before any UPDATE runs.
- `_bulk_edit` (lines 291–326) does `UPDATE ... RETURNING id` but only logs the diff between requested and updated ids — the caller cannot see which ids were skipped, and the pre-queued `memory_sources` list is never pruned.
- `_bulk_insert` (lines 329–363) appends new sources for freshly-inserted memories (safe because the ids come from the just-run INSERT).
- `_bulk_add_sources` (lines 366–394) unconditionally inserts every queued row.

Reviewed `apps/api/src/api/agent/handlers/memory_extraction.py`:

- Lines 208–211 catch `asyncpg.exceptions.ForeignKeyViolationError` from `bulk_apply_memory_operations` and unconditionally raise `TerminalError(f"Chat {chat_id} was deleted, cannot apply memory operations")`, even when the constraint actually violated is `memory_sources_memory_id_fkey`.

Reviewed `apps/api/tests/repositories/test_memory_repository.py` (63 lines): the only coverage is `_bulk_delete` / `_bulk_edit` in isolation, mocking `db.fetch`. No test exercises `bulk_apply_memory_operations` end-to-end or the queued-source-vs-missing-memory interaction.

Reviewed `apps/api/platform/defaults/datastores/migrations/V1__initial_schema.sql:553–583`:

- `memory_sources` has `FOREIGN KEY(memory_id) REFERENCES user_memories(id) ON DELETE CASCADE` and `FOREIGN KEY(chat_id) REFERENCES chats(id) ON DELETE CASCADE`. Postgres will name these `memory_sources_memory_id_fkey` and `memory_sources_chat_id_fkey` (auto-generated names — confirmed by matching pattern in evals data logs, e.g. `stream_events_interaction_id_fkey`).
- `delete_orphaned_memories` trigger deletes a `user_memories` row when its last `memory_sources` row is removed (e.g. via chat cascade). This is the mechanism by which a "valid at read time" memory becomes missing at write time without a direct delete request.

### Run / verify commands (from `justfile`, `apps/api/justfile`, and `.claude/skills/run-code-checks/`)

- Repo-wide lint: `just lint` (or `just api::lint` for backend only).
- API unit tests: `just test-api` (invokes `just api::test` which runs `uv run pytest -v tests/`). To run just the touched test files: `just api::test tests/repositories/test_memory_repository.py tests/ut/test_memory_extractor.py`.
- Full validation via project convention: `/code-checks` (runs lint + UT + integration in parallel).
- No integration tests exist under `tests/integration/` for `user_memories` / `memory_sources`, so unit coverage in `tests/repositories/test_memory_repository.py` is the right place for the new regression tests (matches the acceptance criteria wording: "Add repository/unit coverage").

### Before-state reproduction

The bug is a race between separate Restate steps + an in-batch conflict. It cannot be reliably reproduced by running the app manually — the read/validate/write gap plus the orphan-memory trigger require a specific interleaving. The reproduction that will serve as the "before" baseline is a **failing pytest** that exercises `bulk_apply_memory_operations` against a mocked `db.fetch` returning fewer ids than requested (so `_bulk_edit` records misses), then asserts `_bulk_add_sources` is called with a filtered list. Under current code, the assertion fails (it's called with the full list) — that IS the before-state. This test is included as one of the deliverables in the "Tests to add" section below; before starting implementation, run it once to confirm it fails on `main`.

## Design decisions

### Which of the ticket's suggested fixes to take

The ticket lists three options. We will take **all three**, because they cover independent gaps:

1. **Return updated ids from `_bulk_edit` and filter queued sources before `_bulk_add_sources`.** Covers the external race (memory disappeared between read and write).
2. **Reconcile same-batch delete+edit conflicts in `_split_bulk_operations` with delete-wins semantics.** Covers the internal batch case without depending on `_bulk_edit`'s return. Documented as "delete wins" so the behavior is deterministic and doesn't depend on Postgres execution order.
3. **Inspect `constraint_name` in `memory_extraction.py` before choosing the error message.** After the two fixes above, the memory-id FK should never fire — but if it ever does (defence in depth, or a future regression), the error surfaces accurately instead of pointing at chat deletion.

### Same-batch conflict resolution: delete wins

If both a `DeleteMemoryOperation` and an `EditMemoryOperation` reference the same `memory_id`:

- Drop the edit and any source insert associated with it in `_split_bulk_operations`.
- Log at WARNING with the memory_id so we can spot how often this happens in practice (this should be rare — a signal that the LLM emitted a contradictory batch).

Rationale: delete-wins matches the existing implicit order (`_bulk_delete` runs first), keeps behavior consistent with what the DB would attempt today (edit becomes a no-op), and prevents a stray source insert. The alternative (edit-wins) would require re-inserting after delete, which reshapes the data flow more than needed.

## Concrete changes

### 1. `apps/api/src/api/repositories/memory_repository.py`

#### 1a. `_split_bulk_operations` — drop edits that conflict with deletes in the same batch

Before iterating, collect `delete_ids = {op.memory_id for op in operations if isinstance(op, DeleteMemoryOperation)}`. When appending an `EditMemoryOperation`, skip it (and skip queuing its `MemorySource`) if its `memory_id` is in `delete_ids`; log at WARNING with the memory_id.

Signature can stay as-is (still returns the four lists) — this is a filter, not a signature change.

#### 1b. `_bulk_edit` — return the set of successfully-updated ids

Change return type from `None` to `set[uuid.UUID]`. Build it from `updated_records` (the existing `RETURNING m.id` result — already computed as `updated_ids` local at line 318). Return that set.

Also keep the existing per-id log for missing ids (already there at line 319–324) — that's still useful diagnostic signal.

#### 1c. `bulk_apply_memory_operations` — filter `memory_sources` using the edit result

After `_bulk_edit` returns, compute the set of requested edit ids that were not updated (`edit_ids_requested - updated_ids`). Filter `memory_sources` in place to drop entries whose `memory_id` is in that missing set. Then run `_bulk_insert` (which appends fresh sources for just-inserted memories — always safe) and finally `_bulk_add_sources`.

Sketch:

```python
new_ops, edit_ops, delete_ops, memory_sources = _split_bulk_operations(operations)
async with db.transaction():
    await _bulk_delete(db, entity_id, delete_ops)
    updated_ids = await _bulk_edit(db, entity_id, edit_ops)
    requested_edit_ids = {op.memory_id for op in edit_ops}
    stale_edit_ids = requested_edit_ids - updated_ids
    if stale_edit_ids:
        memory_sources = [s for s in memory_sources if s.memory_id not in stale_edit_ids]
    await _bulk_insert(db, entity_id, new_ops, memory_sources)
    await _bulk_add_sources(db, entity_id, memory_sources)
```

Notes:
- `_bulk_insert` mutates `memory_sources` in place by appending. This is fine because new memories have ids we just chose in `_bulk_insert` — they cannot be in `stale_edit_ids`.
- Update the function docstring: (a) remove the misleading "ForeignKeyViolationError: If a chat is deleted" line and replace with an accurate description that mentions the chat FK is now the only expected FK failure path (memory FK is filtered upstream); (b) note the same-batch delete+edit conflict is resolved delete-wins.

#### 1d. Keep `_bulk_delete` and `_bulk_add_sources` behavior otherwise unchanged

`_bulk_delete` already logs and continues on missing ids — no change. `_bulk_add_sources` doesn't need internal changes; it just receives a pre-filtered list.

### 2. `apps/api/src/api/agent/handlers/memory_extraction.py`

#### 2a. Distinguish which FK actually failed in the `except ForeignKeyViolationError` block

Replace the unconditional "chat deleted" `TerminalError` at lines 208–211 with a branch on `e.constraint_name` (asyncpg's `PostgresError.constraint_name`):

- `memory_sources_chat_id_fkey` → keep the existing `TerminalError(f"Chat {chat_id} was deleted, cannot apply memory operations")`. Terminal is correct: retrying won't help because the chat is gone.
- `memory_sources_memory_id_fkey` → raise a `TerminalError` with an accurate message (e.g. `f"Memory referenced by chat {chat_id} was deleted before write, cannot apply memory operations"`). Also `logger.error` with `exc_info=True` because after the repository fix this should never fire — if it does we want a loud signal.
- Anything else (unexpected constraint) → `logger.error` with `exc_info=True` and re-raise the original `ForeignKeyViolationError` (Restate will treat this as a transient error and retry, which is the correct default for something we don't recognize).

Use exact string comparison against the constraint names; do not use substring matches. Reference for the names: they are Postgres auto-generated from the table + column + `_fkey` suffix (confirmed pattern for the `chats.id` and `user_memories.id` foreign keys defined in `V1__initial_schema.sql:558–559`). If the implementation wants defensive matching, extract them into module-level `_MEMORY_ID_FK = "memory_sources_memory_id_fkey"` / `_CHAT_ID_FK = "memory_sources_chat_id_fkey"` constants at the top of `memory_repository.py` (co-located with the repo that owns the table) and import them from the handler — this keeps the schema knowledge in one place.

### 3. Tests

All new tests go in `apps/api/tests/repositories/test_memory_repository.py` and `apps/api/tests/ut/test_memory_extractor.py`. No new integration test (matches existing pattern; memory integration coverage is not present today, and the fix is repository-layer logic).

#### 3a. `test_memory_repository.py` — new tests

Follow the existing `TestMemoryRepositoryBulkMissingTargets` mock style (`AsyncMock` for `db`, patch `logger`).

- **`test_bulk_apply_memory_operations__filters_sources_for_missing_edit_target`** — full `bulk_apply_memory_operations` path. Two edit ops (one existing id, one missing). Mock `db.fetch` to return one row for the edit UPDATE (existing id only), and empty for the source INSERT. Assert:
  - No `ForeignKeyViolationError` raised.
  - The INSERT INTO team_memory_sources call was made with only 1 memory_id (the existing one), not 2.
  - `logger.error` was called for the missing id (from the existing `_bulk_edit` log).

- **`test_bulk_apply_memory_operations__delete_wins_when_same_id_has_edit`** — one batch with `DeleteMemoryOperation(id=X)` + `EditMemoryOperation(id=X, ...)`. Assert:
  - The edit call is either not issued or issued with 0 ids (so the DB never receives an UPDATE for the deleted id).
  - No source insert is issued for id X.
  - A WARNING/ERROR log fires naming the conflicting memory_id.
  - No `ForeignKeyViolationError` raised.

- **`test_split_bulk_operations__drops_edits_whose_id_is_also_deleted`** — direct unit test of `_split_bulk_operations` (pure function, no db) confirming that when the input contains both a delete and an edit for id X, the returned `edit_ops` excludes X and `memory_sources` excludes X.

- **`test_bulk_edit__returns_updated_ids`** — direct unit test confirming the new return value contains only the ids present in `db.fetch`'s return set.

- Keep the existing `test_bulk_delete__logs_missing_ids_without_raising` and `test_bulk_edit__logs_missing_ids_without_raising`. The latter must be updated to also assert the returned set now, since `_bulk_edit`'s signature changed.

#### 3b. `test_memory_extractor.py` — new tests in `TestMemoryExtractorHandler`

Follow the existing `mock_bulk_apply_memory_operations` fixture pattern.

- **`test_update_memories__raises_chat_deleted_terminal_on_chat_fk`** — `mock_bulk_apply_memory_operations.side_effect` raises `ForeignKeyViolationError` with `constraint_name="memory_sources_chat_id_fkey"`. Assert `TerminalError` with message containing `"Chat {chat_id} was deleted"`.

- **`test_update_memories__raises_memory_deleted_terminal_on_memory_fk`** — same but `constraint_name="memory_sources_memory_id_fkey"`. Assert `TerminalError` whose message does NOT say "Chat was deleted" and mentions memory instead.

- **`test_update_memories__reraises_unknown_fk_violation`** — `constraint_name="something_else_fkey"`. Assert the original `ForeignKeyViolationError` bubbles up (not wrapped as `TerminalError`).

Constructing a `ForeignKeyViolationError` with a specific `constraint_name` in tests: asyncpg PostgresError subclasses expose `constraint_name` as a read-only attribute derived from `_asyncpg_serverfields`. The idiomatic way in tests is to instantiate via the private classmethod, or simpler, use `unittest.mock.MagicMock(spec=ForeignKeyViolationError, constraint_name="...")`. Either works — pick whichever plays nicer with `raise` (a real subclass instance may be needed). If MagicMock doesn't `raise` cleanly, construct with `ForeignKeyViolationError("msg")` and monkeypatch `.constraint_name` via `object.__setattr__`, or use a small subclass in the test module.

## Ordering of changes

Dependencies mean the repository changes must land in the order listed:

1. `_bulk_edit` return-type change (1b).
2. `_split_bulk_operations` conflict filter (1a) — no dependency, can go first, but ordering above keeps callers consistent.
3. `bulk_apply_memory_operations` filter step (1c) — depends on (1b).
4. `memory_extraction.py` constraint-name branch (2a) — independent, land alongside (1) so error surface is consistent.
5. Tests (3) — land in the same PR. Confirm the "before-state" tests (`__filters_sources_for_missing_edit_target`, `__delete_wins_when_same_id_has_edit`) fail on `main` before the fix, then pass after.

## Edge cases & risks

- **Empty batches / empty edit_ops** — `_bulk_edit` currently early-returns before running the query. It must still return an empty set in that branch (not `None`) to keep the caller's type contract clean.
- **All edit targets missing** — `stale_edit_ids == requested_edit_ids`; `memory_sources` becomes empty for the edit portion; `_bulk_insert` may still append new-memory sources; `_bulk_add_sources` runs on whatever remains (or no-ops if empty).
- **Duplicate edit ops for the same id in one batch** — the LLM shouldn't do this, but if it did, `_split_bulk_operations` currently appends duplicate `MemorySource` entries. `_bulk_add_sources` uses `ON CONFLICT DO NOTHING`, so this is already benign; our filter doesn't change that. Not in scope to dedupe.
- **Transaction rollback semantics** — if `_bulk_add_sources` still raises (e.g. chat FK because chat was cascade-deleted), the outer `async with db.transaction()` rolls back including the successful deletes/edits/inserts. That's the same behavior as today, and the handler's `TerminalError` conversion means the Restate step surfaces a stable failure. Confirm the transaction wrapper (line 228) stays as-is.
- **`memory_extraction.py` line 152–155** — separately raises `RuntimeError` for LLM-generated invalid ids (validated against the read snapshot). This is a different (earlier) protection and is out of scope; not touching it.
- **`delete_orphaned_memories` trigger** — schema-side, explicitly out of scope per the ticket. Behavior stays as-is; our application-level filter makes it safe.
- **Constraint-name string drift** — Postgres auto-generates names as `<table>_<column>_fkey`. Renaming the table or column in a future migration would silently break the branch. Mitigation: extract to module-level constants (see 2a) so a grep hits both sites, and reference them from tests too.

## Verification

- **`just api::test tests/repositories/test_memory_repository.py tests/ut/test_memory_extractor.py`** — new tests pass; existing tests still pass.
- **`just api::lint`** — ruff + ruff format + ty pass (touched files: `memory_repository.py`, `memory_extraction.py`, two test files).
- **`/code-checks`** — full run: lint + all UTs + integration tests (integration surface unaffected by this change, so should be a green baseline).
- **Observable before/after via tests, per acceptance criteria:**
  - Before: `test_bulk_apply_memory_operations__filters_sources_for_missing_edit_target` fails on `main` (source INSERT called with 2 ids).
  - After: same test passes; source INSERT called with only the existing id.
  - Before: `test_update_memories__raises_memory_deleted_terminal_on_memory_fk` fails on `main` (message says "Chat was deleted").
  - After: same test passes with an accurate message.

## Files touched

- `apps/api/src/api/repositories/memory_repository.py` — logic changes 1a–1d, updated docstring, optional module-level FK constants.
- `apps/api/src/api/agent/handlers/memory_extraction.py` — updated `except ForeignKeyViolationError` block (2a), optional import of the FK constants.
- `apps/api/tests/repositories/test_memory_repository.py` — new tests 3a; existing `test_bulk_edit__logs_missing_ids_without_raising` updated for new return type.
- `apps/api/tests/ut/test_memory_extractor.py` — new tests 3b under `TestMemoryExtractorHandler`.

## Out of scope (per ticket)

- Schema/trigger changes to `V1__initial_schema.sql`.
- Broader read-validate-write race hardening beyond the two specific cases here.
- LLM-side de-duplication of contradictory operations (delete+edit for same id) — handled deterministically at the repository, not by prompting the LLM differently.
- Integration test scaffolding for user_memories (none exists today; repository/unit coverage is sufficient per the acceptance criteria).
