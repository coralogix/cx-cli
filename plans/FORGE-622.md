# FORGE-622 — DB constraint: one enabled `watch_dashboard` task per dashboard

## Environment blocker (read first)

**The project cannot be run or tested in this worktree.** Verified:
- `docker`, `just`, `dotenvx` are not installed (only `uv`); no `.env` file (only `.env.example` / `.env.development` / `.env.test`), so `just dev` / `just test-integration` cannot start Postgres or the API.
- `uv sync --all-extras` fails: `No space left on device` (`/data` overlay is 100% full, ~690MB free after I removed the partial `.venv` I had created).

So **no before-state was captured at runtime**; the plan below is grounded in reading the code. The implementation step must run the checks listed in "Verification" on a machine with Docker + a synced venv, and capture before/after artifacts there (`.saga/artifacts/`): e.g. `curl`/httpx output of two consecutive `POST /scheduled-tasks/` for the same dashboard, showing today's `201 + 201` vs. the new `201 + 409 {"detail": "<indicative message>"}`.

## Current behavior (from code)

- `watch_dashboard` tasks store `metadata` JSONB `{"dashboard_url": ..., "dashboard_id": ...}` — `dashboard_id` is a Pydantic `computed_field` on `WatchDashboardMetadata` (`libs/common/src/common/schemas/scheduled_tasks_schema.py:50-68`), so it is always present in `model_dump(mode="json")` and therefore always in the stored JSONB.
- Only unique index on the table today: `uq_scheduled_tasks_team_entity_title (team_id, entity_id, title)` (`V29__scheduled_task_table_fixes.sql`). Nothing prevents N enabled watch tasks on the same dashboard.
- `create_scheduled_task_entry` (`apps/api/src/api/repositories/scheduled_task_repository.py:149-168`) maps **any** `UniqueViolationError` to `AlreadyExistsException`, whose global handler (`libs/common/src/common/exception_handlers.py:48-55`) discards the message and returns `{"detail": "Resource already exists"}`.
- `update_scheduled_task_entry` (same file, `:259-263`) maps `UniqueViolationError` to `AlreadyExistsException` **after `assert title is not None`** — a unique violation on a non-title update currently raises `AssertionError` → 500.
- Reads are entity-scoped: `list_scheduled_tasks_for_dashboard` filters `entity_id + kind + metadata->>'dashboard_id'` (`:59-79`), exposed as `GET /scheduled-tasks/for-dashboard/{dashboard_id}` — this is what the FE watch button uses to decide its state.
- Precedent for an indicative 409: `ScheduledTaskQuotaExceededError` (`apps/api/src/api/exceptions.py:59-70`) caught in the route (`scheduled_tasks_route.py:95-96`) → `HTTPException(409, detail=exc.reason)`.
- Creates are wrapped in a transaction with a per-`(team_id, entity_id)` advisory lock (`service.py:125-152`).

## Decision: uniqueness scope = `(team_id, entity_id, dashboard_id)` (per user), not team-wide

The ticket text suggests `(team_id, dashboard_id)`. I am deviating deliberately — **flag this in the PR description and let the assignee override if product wants team-wide**:
- Every existing read path is entity-scoped (`for-dashboard` route + repo `:59-79`), and the existing title uniqueness is `(team_id, entity_id, title)`.
- FORGE-577's FE derives the watch-button state from the entity-scoped `for-dashboard` list. With a team-wide constraint, user B on a dashboard already watched by user A would see an "unwatched" button that always 409s — a broken state the FE ticket does not cover.

Switching to team-wide later is a one-line change in the index definition + the corresponding test; keep the index name unchanged either way.

## Changes, in order

### 1. Alembic migration (dependency for everything else)

`just db-create-migration "add_watch_dashboard_unique_index"` → new file in `apps/api/alembic_migrations/versions/` with `down_revision = "d3f2a91b6e04"` (current head: `d3f2a91b6e04_migrate_scheduled_tasks_to_gpt_5_6_terra.py`).

`upgrade()` — two steps, in this order, both `op.execute`:

1. **Dedupe pre-existing data** (index creation fails otherwise). Keep the earliest-created enabled watch task per group; disable the rest:
```sql
WITH ranked AS (
    SELECT id, ROW_NUMBER() OVER (
        PARTITION BY team_id, entity_id, metadata->>'dashboard_id'
        ORDER BY created_at, id
    ) AS rn
    FROM scheduled_tasks
    WHERE enabled = true
      AND kind = 'watch_dashboard'
      AND metadata->>'dashboard_id' IS NOT NULL
)
UPDATE scheduled_tasks t
SET enabled = false, next_run_at = NULL, updated_at = now()
FROM ranked r
WHERE t.id = r.id AND r.rn > 1;
```
2. **Create the partial expression unique index**:
```sql
CREATE UNIQUE INDEX uq_scheduled_tasks_watch_dashboard
ON scheduled_tasks (team_id, entity_id, (metadata->>'dashboard_id'))
WHERE enabled = true AND kind = 'watch_dashboard';
```

`downgrade()`: `DROP INDEX IF EXISTS uq_scheduled_tasks_watch_dashboard;` (the data disable is not reversible — say so in a comment).

Notes:
- **Do not** use `refresh_view` / drop-recreate `team_scheduled_tasks`: this migration adds no columns and does not alter the table shape, so the view is unaffected (contrast `V29`/`V45`, which changed columns).
- `CONCURRENTLY` is not usable (Alembic runs migrations in a transaction). The table is small; a brief `ACCESS EXCLUSIVE` lock is acceptable.
- Rows disabled by step 1 keep their Restate schedule, but `run_due_task` returns early for `enabled=false` (`service.py:354-355`), so they simply stop running. Mention in the migration comment.
- Rows whose `metadata->>'dashboard_id'` is NULL never conflict (NULLs are distinct in a unique index) — safe.

Then run `just db-generate-migrations` — it regenerates the whole `apps/api/platform/defaults/datastores/migrations/` dir, producing `V49__add_watch_dashboard_unique_index.sql`. Commit the generated SQL.

### 2. New exception — `apps/api/src/api/exceptions.py`

Mirror `ScheduledTaskQuotaExceededError`:
```python
class ScheduledTaskDashboardConflictError(Exception):
    """Raised when an enabled watch_dashboard task already exists for a dashboard.

    Mapped to HTTP 409 by the route layer.
    """

    def __init__(self, dashboard_id: str | None = None) -> None:
        target = f"dashboard '{dashboard_id}'" if dashboard_id else "this dashboard"
        self.dashboard_id = dashboard_id
        self.reason = (
            f"An active watch task for {target} already exists. "
            "Disable or delete the existing task before creating a new one."
        )
        super().__init__(self.reason)
```

### 3. Repository — `apps/api/src/api/repositories/scheduled_task_repository.py`

- Add a module constant `WATCH_DASHBOARD_UNIQUE_INDEX = "uq_scheduled_tasks_watch_dashboard"` (asyncpg populates `exc.constraint_name` with the index name for unique-index violations).
- `create_scheduled_task_entry` (`:165-168`): branch before the generic mapping —
```python
except asyncpg.UniqueViolationError as exc:
    if exc.constraint_name == WATCH_DASHBOARD_UNIQUE_INDEX:
        dashboard_id = (
            metadata.dashboard_id if isinstance(metadata, WatchDashboardMetadata) else None
        )
        raise ScheduledTaskDashboardConflictError(dashboard_id) from exc
    raise AlreadyExistsException(f"A scheduled task titled '{title}' already exists.") from exc
```
- `update_scheduled_task_entry` (`:259-263`): same branch first (no `dashboard_id` in scope → pass `None`), and **replace the `assert title is not None`** with a safe fallback so a non-title unique violation can never 500:
```python
except asyncpg.UniqueViolationError as exc:
    if exc.constraint_name == WATCH_DASHBOARD_UNIQUE_INDEX:
        raise ScheduledTaskDashboardConflictError() from exc
    message = (
        f"A scheduled task titled '{title}' already exists."
        if title is not None
        else "A scheduled task with these details already exists."
    )
    raise AlreadyExistsException(message) from exc
```
This path matters: FORGE-577's FE disables (rather than deletes) a watch task and re-enables it later via `PUT /{task_id}/schedule` — re-enabling while another enabled task exists for the same dashboard now hits the index.

No pre-check query in the service: the DB index stays the single source of truth (avoids TOCTOU and a second code path).

### 4. Routes — `apps/api/src/api/routes/scheduled_tasks_route.py`

- `create_scheduled_task` (`:78-96`): add, after the existing handlers,
  `except ScheduledTaskDashboardConflictError as exc: raise HTTPException(status_code=HTTPStatus.CONFLICT, detail=exc.reason) from exc`.
- `update_scheduled_task_schedule` (`:118-134`): add the same `except` (this is the re-enable path).
- Leave `update_scheduled_task_content` alone (it cannot flip `enabled`).

### 5. Agent tool — `apps/api/src/api/agent/agents/skills_agent/tools/scheduled_tasks_tools.py`

`create_scheduled_task` (tool, `:284+`) calls the service directly, so the new exception would surface as an unhandled tool error. Wrap the `scheduled_tasks_service.create_scheduled_task(...)` call and re-raise as `ToolError(exc.reason)` (`api.agent.shared.exceptions.ToolError`, already imported and used at `:62`) so the model gets an actionable message ("a watch already exists — offer to open/manage it") rather than a stack trace.

### 6. Tests

**Integration — `tests/integration/test_scheduled_tasks_route.py`** (extend; reuse `_insert_watch_dashboard_task` at `:132` and the `_valid_create_payload` helper with `kind="watch_dashboard"` + `metadata={"dashboard_url": "https://app.coralogix.com/#/dashboards/<id>"}`):
1. First watch create for a dashboard → `201`.
2. Second enabled watch create for the same dashboard → `409`, and `body["detail"] != "Resource already exists"` and contains the indicative wording (assert on a substring like `"already exists"` + dashboard reference; the point is it is *not* the generic body).
3. After the first task is disabled (`PUT /{id}/schedule` with `enabled: false`), creating a new watch task for the same dashboard → `201`.
4. Two different dashboards → both `201`.
5. Re-enable path: with an enabled watch task for dashboard X, `PUT /{other_task_id}/schedule` `enabled: true` on a disabled task for the same X → `409` (explicitly assert **not** 500 — guards the removed `assert`).
6. Isolation: an enabled watch task for the same dashboard inserted for a **different `entity_id`** (and, separately, a different `team_id`, mirroring `test_list_scheduled_tasks_for_dashboard__other_team_invisible`) does not block creation.
7. Regression: multiple `standard` tasks (metadata NULL) still create fine.

**Unit — `apps/api/tests/ut/test_scheduled_tasks_repository.py`**: fake `asyncpg.UniqueViolationError` with `constraint_name = "uq_scheduled_tasks_watch_dashboard"` → `ScheduledTaskDashboardConflictError` (with `dashboard_id` populated on the create path); with `constraint_name = "uq_scheduled_tasks_team_entity_title"` → `AlreadyExistsException`; update path with `title=None` → `AlreadyExistsException`, not `AssertionError`.

**OpenAPI**: run `just openapi-generate` and commit `apps/api/openapi.json` if the response set changes.

## Risks / edge cases

- **Constraint-name detection**: if `exc.constraint_name` is unexpectedly empty, the code falls back to the generic title message. Confirm the integration test for case 2 actually asserts the indicative body — that test is what proves the detection works end-to-end.
- **Pre-existing duplicates in prod** are silently disabled by the migration. If that is unacceptable, the alternative is failing the migration loudly — but that blocks deploys. Keep the disable and call it out in the PR description.
- **Per-user vs per-team scope** — see Decision above; flag in the PR.
- **Migration order**: the dedupe UPDATE must run before `CREATE UNIQUE INDEX` in the same transaction.
- Do not modify any existing migration file (project rule).

## Verification

Run commands (need Docker + a synced venv; not available in this worktree):
- Dev env: `just dev` (runs `db-migrate` first) — or `/run-dev-env`.
- Migration: `just db-migrate` then `just db-generate-migrations` (must be re-run after any migration edit); optionally `just db-verify-tables`.
- Lint/typecheck (scoped): `just api::lint` and `just common::lint` (or `just lint-fix`).
- Unit: `just test-api tests/ut/test_scheduled_tasks_repository.py tests/ut/test_scheduled_tasks_service.py`.
- Integration: `just tests::tests-environment-up` then `just test-integration integration/test_scheduled_tasks_route.py`.
- Full gate: `/code-checks`.

Behavior to observe and capture as artifacts:
- **Before**: two consecutive `POST /scheduled-tasks/` with `kind=watch_dashboard` and the same `dashboard_url` → both `201`.
- **After**: first `201`, second `409` with the indicative `detail`; disable the first, then a third create → `201`.
- **DB check**: `\d+ scheduled_tasks` (or `SELECT indexdef FROM pg_indexes WHERE indexname = 'uq_scheduled_tasks_watch_dashboard'`) shows the partial expression index.
