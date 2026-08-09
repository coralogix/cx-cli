# FORGE-587 — Console URLs in `olly` MCP write-tool responses

## 0. Environment: how to run / check (verified in this worktree)

`just` and `protoc` are **not installed** here, and `libs/common/src/common/generated` ships empty — tests fail to collect until protos are compiled. Sequence that works (run once, before anything else):

```bash
cd libs/common
sh ./scripts/fix-audit-log-conflict.sh
sh ./scripts/generate-proto.sh
# `just _proto-fix` needs a `protoc` binary; shim it onto grpc_tools:
mkdir -p /tmp/shim && printf '#!/bin/sh\nexec python -m grpc_tools.protoc "$@"\n' > /tmp/shim/protoc && chmod +x /tmp/shim/protoc
PATH=/tmp/shim:$PATH uv run protol --in-place --python-out=src/common/generated protoc --proto-path=proto $(find proto -name '*.proto')
```
(`proto/` and `src/common/generated/` are gitignored — `git status` stays clean.)

Checks:

```bash
# libs/common  (equivalent of `just common::test` / `just common::lint`)
cd libs/common && uv run pytest -q
cd libs/common && uv run ruff check . && uv run ruff format --check . && uv run ty check

# apps/ws-ai-mcp  (separate uv project + lockfile; settings need env)
cd apps/ws-ai-mcp && env CORALOGIX_SCHEMA_STORE_ADDRESS=localhost:9090 CX_ID=stg1 \
  OAUTH_CLIENT_ID=x OAUTH_CLIENT_SECRET=y JWT_SIGNING_KEY=abc \
  BASE_URL=https://example.com/mcp_oauth REDIS_HOST=localhost REDIS_PORT=6380 \
  REDIS_OAUTH_DB=0 MCP_PORT=8081 IS_LOCAL_DEVELOPMENT=true uv run pytest -q
cd apps/ws-ai-mcp && uv run ruff check . && uv run ruff format --check . && uv run ty check src/mcp_server
```

CI: `.github/workflows/mcp_ci.yaml` runs `just test` + `just smoke` for ws-ai-mcp; `ci.yaml` `common-ut` runs `just test-common`; `ci.yaml` `lint` job lints ws-ai-mcp.

**Baseline (before-state) observed and captured** to `.saga/artifacts/mcp-write-tools-before.json` by driving the real impls with stub clients:
- `manage_dashboards(create)` → `{"dashboardId": "dash-123"}`
- `manage_cases(acknowledge)` → `{"case": {"id": "case-abc", "status": "ACK"}}`
- `manage_views(create)` → `{"view": {"id": "view-9", "name": "My view"}}`

No `consoleUrl` anywhere. Baseline test runs: `libs/common` targeted suites 42 passed; `apps/ws-ai-mcp/tests/tools/{dashboards_kb,cases,views}` 13 passed.

## 1. Key findings that shape the design (read `src/console_url.rs`, `src/identity.rs`, `src/execution.rs`, `src/commands/{dashboards,cases,views}/mod.rs` from `coralogix/cx-cli@saga/forge-586-cli`, plus PR #176 body)

1. **Base-URL resolution is `GET {api_base}/identity/whoami` → `team_url`, used verbatim** (trailing slash trimmed). cx-cli does *not* build the console host from a region→domain map. `whoami` returns e.g. `{"team_id": 53623, "team_name": "c4c", "team_url": "https://c4c.app.eu2.coralogix.com"}`.
   → **Do not** use `CORALOGIX_APP_ENV_TO_DOMAIN` / `resolve_frontend_team_url` (`apps/api/src/api/utils/urls.py:16`). That path requires `CoralogixHeaderAuth` + an in-cluster identity gRPC channel (`common/clients/identity_grpc.py`, `grpc.aio.insecure_channel(IDENTITY_SERVICE_URL)`); the MCP has neither (`CoralogixApiKeyAuth`, no `IDENTITY_SERVICE_URL` in `mcp_server/settings.py`). whoami also removes the need for a region map entirely, which covers `factset`/`proofpoint`/custom regions for free.
2. **No `#/` hash prefix.** `console_url.rs`'s module docs state the console dropped hash routing (only `/grafana`, `/opendashboards`, `/login*` still use the fragment) and every builder is a plain path. So **do not** reuse `api/utils/urls.py::create_frontend_link` (it force-inserts `#/`). Routes to use:
   - dashboard → `{base}/dashboards/{id}`
   - case → `{base}/cases?id={urlencoded id}`
   - saved view → `{base}/explore?viewId={urlencoded id}`
   - AI Center evaluations → `{base}/ai-center/overview/eval-catalog`
3. **Folders get no link** — cx-cli deliberately omits dashboard-folder and view-folder links (`folderId` never appears in a console URL). This is a **deviation from the ticket's success-criteria bullet for `manage_dashboard_folders create`**; carry it, and call it out in the PR description per the parent ticket's skip-and-report rule.
4. **Tagging shape**: cx-cli's `render::tag_console_url` inserts `consoleUrl` **inside** the wrapper when the payload is an object with exactly one key whose value is itself a non-empty object (`{"case": {...}}` → `{"case": {..., "consoleUrl": ...}}`); otherwise at the root; no-op for non-objects, empty objects, and empty wrappers. Mirror this exactly — MCP case/view responses are wrapper-shaped, dashboard create returns flat `{"dashboardId": "..."}`.
5. **The `*_impl` functions in `libs/common/src/common/tools/` are consumed only by the MCP** (verified: `apps/api` does not import them). Signature changes are MCP-only, but keep the new parameter optional so existing tests/callers are unaffected.
6. Auth is compatible: `CoralogixApiKeyAuth.get_http_auth_headers()` emits `Authorization: Bearer <token>` + `cgx-team-id`, the same scheme cx-cli uses against `/identity/whoami`.

## 2. Changes, in dependency order

### Step 1 — `libs/common/src/common/utils/console_url.py` (new, pure, no I/O)

```python
ConsoleBaseResolver = Callable[[], Awaitable[str | None]]

def trim_base(base: str) -> str                      # strip trailing "/"
def dashboard_url(base: str, dashboard_id: str) -> str
def case_url(base: str, case_id: str) -> str          # urllib.parse.quote(..., safe="")
def view_url(base: str, view_id: str) -> str
def ai_center_evaluations_url(base: str) -> str
def id_from_payload(payload: object, pointers: Sequence[str]) -> str | None
def tag_console_url(payload: object, url: str) -> None   # in-place, mirrors cx-cli nesting rules
async def resolve_and_tag(payload, resolver, build) -> None  # best-effort; swallows all exceptions
```

- `id_from_payload` accepts JSON-pointer-ish dotted paths and coerces `str`/`int` ids to `str` (cx-cli accepts numeric ids).
- `resolve_and_tag` is the single choke point: `if resolver is None or entity_id is None: return`; `try: base = await resolver() except Exception: log.debug + return`; `if not base: return`; then `tag_console_url(payload, build(base))`. **A console link must never fail or slow a successful write.**

### Step 2 — `libs/common/src/common/clients/coralogix_client.py`: add `whoami()`

```python
WHOAMI_PATH = "/identity/whoami"

async def whoami(self, timeout: float = 5.0) -> dict:
    """Identify the team behind the current credentials (team_id/team_name/team_url)."""
```
Use the existing `_make_get_request`-style header construction but with an explicit short timeout (the class default is 60s and must not gate a write response). Add `timeout` plumbing rather than reusing `self.timeout`.

### Step 3 — `apps/ws-ai-mcp/src/mcp_server/console_url.py` (new)

```python
async def resolve_console_base(ctx: Context) -> str | None
```
- Short-circuit on an optional `settings.console_url` override (new optional `Settings` field, env `CONSOLE_URL`, default `None`) — mirrors cx-cli's profile override and makes local/e2e testing trivial.
- Otherwise: `company_id = (await ctx.get_state("user_info") or {}).get("company_id")`; consult a module-level `cachetools.TTLCache` keyed on `company_id` (positive TTL ~1h, negative/`None` TTL ~60s so failures aren't retried per call); on miss call `(await get_coralogix_client(ctx)).whoami()` and take `team_url`, `rstrip("/")`, empty → `None`.
- Never raises; logs at debug/warning via `mcp_server.middleware.mcp_logger`.
- Precedent for async caching already in-repo: `mcp_server/tools/coralogix_docs/coralogix_docs.py` uses `from cache import AsyncTTL`; `cachetools` + `cachetools-async` are already declared deps. Either is fine — prefer an explicit `TTLCache` keyed on `company_id` so the client object isn't part of the cache key.

### Step 4 — thread the resolver through the shared impls

Add `console_base_resolver: ConsoleBaseResolver | None = None` (keyword-only, default `None`) to:

| Function | File | Tag when |
|---|---|---|
| `manage_dashboard_impl` | `libs/common/src/common/tools/dashboards_kb_tools.py:150` | action `CREATE` / `REPLACE` only. id from `dashboardId` → `id` → `dashboard.id`; for `REPLACE` fall back to `body["dashboard"]["id"]` (cx-cli does this — some deployments return an empty body). **Not** `CHECK` (no persisted id in the MCP surface — the MCP's `check` takes a body, never an id) and **not** `DELETE`. |
| `manage_views_impl` | `libs/common/src/common/tools/views_tools.py:132` | action `CREATE` (id from `view.id` → `id`) and `UPDATE` (use the `view_id` argument). Not `DELETE`. |
| `manage_cases_impl` | `libs/common/src/common/tools/cases_tools.py:35` | every action that resolves a single `case_id` — `UPDATE`, `COMMENT`, `ASSIGN`, `UNASSIGN`, `ACKNOWLEDGE`, `UNACKNOWLEDGE`, `RESOLVE`, `CLOSE`, `SET_PRIORITY`, `CLEAR_PRIORITY`, and `EVENTS`. Use `case_ids[0]` (the id is always known from the request, never parsed from the response). Not `GET_EVENT`, not `NOTIFICATIONS` (multi-case). Note `COMMENT` and `EVENTS` go beyond the ticket's list but match cx-cli. |
| `manage_dashboard_folders_impl` | `libs/common/src/common/tools/dashboards_kb_tools.py:190` | **no change** (see §1.3) |
| `manage_view_folders_impl` | `libs/common/src/common/tools/views_tools.py:85` | **no change** |

Refactor each to build the response `dict`, call `await resolve_and_tag(...)`, then `json.dumps(..., indent=2)`. Keep every existing validation/error path byte-identical.

### Step 5 — MCP tool wrappers pass the resolver

- `apps/ws-ai-mcp/src/mcp_server/tools/dashboards/dashboards_kb_tools.py:58` (`manage_dashboards_tool`)
- `apps/ws-ai-mcp/src/mcp_server/tools/views/views_tools.py:130` (`manage_views_tool`)
- `apps/ws-ai-mcp/src/mcp_server/tools/cases/cases_tools.py:35` (`manage_cases_tool`)

each adds `console_base_resolver=lambda: resolve_console_base(ctx)`. Resolution stays lazy — nothing is called unless a write succeeded with a known id.

### Step 6 — AI Center (in scope: MCP *does* expose write tools here)

In `apps/ws-ai-mcp/src/mcp_server/tools/ai_center/ai_center_tools.py`, tag `manage_ai_evaluation` (`create`/`update`, not `delete`) and `manage_ai_custom_evaluation` (`create`/`update`/`add_policy`/`remove_policy`) with `ai_center_evaluations_url(base)`. **Skip `manage_ai_model_pricing`** — cx-cli confirmed model pricing is dialog-only with no route. These tools bypass the `libs/common` impls, so tag inside the local `_run` helper (add an optional post-processing hook rather than duplicating the JSON round-trip in each branch).

### Step 7 — tool descriptions

Add one sentence to `MANAGE_DASHBOARDS_TOOL_DESCRIPTION_V2`, `MANAGE_VIEWS_*`, `MANAGE_CASES_*` in `apps/ws-ai-mcp/src/mcp_server/tools_description_v2.py`: the response may include a `consoleUrl` field linking to the entity in the Coralogix console, which the agent should surface to the user. Without this the model has no reason to relay the link.

## 3. Tests

New — `libs/common/tests/test_console_url.py`:
- each builder's exact output, including trailing-slash trimming and URL-encoding of ids containing `/`, `?`, `&`, spaces;
- `tag_console_url`: flat multi-key object → root; single-key wrapper whose value is a non-empty object → nested; single-key scalar (`{"dashboardId": "x"}`) → root; single-key list → root; empty root → no-op; empty wrapper → no-op; non-dict → no-op;
- `id_from_payload`: first-pointer-wins, numeric-id coercion, missing → `None`;
- `resolve_and_tag`: resolver `None` / returns `None` / returns `""` / raises → payload untouched, no exception.

Extend `libs/common/tests/test_dashboards_kb_tools.py`, `test_cases_tools.py`, `test_views_tools.py` (all existing tests must stay green unchanged — the new kwarg defaults to `None`):
- create/replace/update with a resolver → `consoleUrl` present at the expected JSON path with the expected URL;
- delete + folder actions + `check` + `get_event` + `notifications` → no `consoleUrl`;
- resolver raising → response identical to today and no exception propagated;
- response missing an id → no `consoleUrl` (and for dashboard `replace`, the `body`-id fallback still produces one).

New/extended in `apps/ws-ai-mcp/tests/`:
- `tests/test_console_url.py`: override short-circuits whoami; whoami success → trimmed `team_url`; whoami failure/missing `team_url` → `None`; positive and negative results are cached (client called once for two calls); missing `company_id` → `None`.
- extend `tests/tools/test_{dashboards_kb,cases,views}_tools.py` (they already monkeypatch the impls) to assert `console_base_resolver` is passed through and is not invoked eagerly.

Add a `libs/common` test for `CoralogixClient.whoami()` (URL built as `{api_url}/identity/whoami`, auth headers present, JSON returned) alongside the existing client tests.

## 4. Verification (after-state)

1. Both check suites in §0 green.
2. Regenerate the before/after artifact: re-run the same stub-client script with a resolver returning `https://c4c.app.eu2.coralogix.com`, write to `.saga/artifacts/mcp-write-tools-after.json`, and confirm the diff versus `mcp-write-tools-before.json` is exactly the added `consoleUrl` fields:
   - `{"dashboardId": "dash-123", "consoleUrl": "https://c4c.app.eu2.coralogix.com/dashboards/dash-123"}`
   - `{"case": {"id": "case-abc", "status": "ACK", "consoleUrl": ".../cases?id=case-abc"}}`
   - `{"view": {"id": "view-9", "name": "My view", "consoleUrl": ".../explore?viewId=view-9"}}`
3. **Live check (needs credentials this environment does not have):** with a real key, `curl -H "Authorization: Bearer $KEY" https://api.<region>.coralogix.com/identity/whoami` must return a `team_url`. If that fails for the MCP's OAuth-derived tokens (as opposed to raw API keys), the whole feature degrades to "no link" silently — acceptable, but it must be checked before merge, e.g. via `just smoke` (`MCP_SMOKE_API_KEY`) extended with a `manage_*` dry check, or by hand against staging.

## 5. Risks / edge cases

- **Hash prefix.** cx-cli's builders emit plain paths (`/dashboards/{id}`); PR #176's coverage table text still shows `#/dashboards/{id}` (stale), and olly's own `create_frontend_link` forces `#/` while `get_alert_base_url`/`get_skill_base_url` do not. Follow cx-cli's *source* (no `#/`) — its module docs cite the console's `HostedAppLocationStrategy` carve-out list — but confirm one URL by hand in a browser before merge. Cheap to flip if wrong.
- **Extra HTTP call per successful write.** Mitigated by lazy resolution + TTL cache + 5s timeout + swallow-all. Never resolve on validation errors, deletes, reads, or id-less responses.
- **Wrong-team link.** Cache is keyed on `company_id` from the request's auth context, so a shared process cannot leak one team's `team_url` to another. Do not key on anything token-derived-but-not-team-scoped.
- **Response-shape drift.** Tagging is defensive: any non-dict / empty payload is a no-op, so an unexpected backend shape yields no link rather than a malformed one.
- **Contract change for MCP clients.** Adding a field to a JSON string response is additive; existing agent prompts/tests that assert exact JSON equality on these three tools would break — the extended tests above cover the ones in this repo.

## 6. PR description must call out (parent ticket's skip-and-report rule)

- Dashboard folders and view folders: **no** console link — `folderId` never appears in a console route (confirmed in cx-cli's frontend-source audit).
- `manage_dashboards` action `check`: no link, because the MCP's `check` only validates a body and has no persisted entity id (cx-cli links `check <id>` only in its by-id form).
- `manage_ai_model_pricing`: no link — dialog-only, no route.
- Alerts: read-only in the MCP today, so nothing to link (as the ticket states).
- Read tools (`search_dashboard --id`, `get_view`, `get_view_folder`): deliberately not linked in this round; cx-cli links `dashboards get`/`alerts get` but leaves `views get` as backlog. Follow-up if wanted.
- Any team whose `/identity/whoami` returns no `team_url` (self-hosted/custom deployments): silently no link.
