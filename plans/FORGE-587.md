# FORGE-587 — `consoleUrl` in MCP write-tool responses

Add a `consoleUrl` field to the JSON responses of the mutating dashboard / view / case `olly` MCP tools, built by **extending the existing `resolve_frontend_team_url` / `CORALOGIX_APP_ENV_TO_DOMAIN` machinery** (`apps/api/src/api/utils/urls.py:16-42`) so it is usable from `apps/ws-ai-mcp`, mirroring cx-cli PR #176.

---

## 0. Environment: how to run and check this project

`just` is **not installed** in this worktree, neither is `protoc`, and `libs/common` cannot be imported at all until protobufs are generated. Verified bootstrap:

### 0.1 One-time bootstrap (required before any test run)

```bash
cd libs/common
sh ./scripts/fix-audit-log-conflict.sh     # resolves duplicate audit-log proto symbols
sh ./scripts/generate-proto.sh             # protofetch + grpc_tools.protoc -> src/common/generated/

# `protol` (the _proto-fix step) shells out to a `protoc` binary that is absent. Shim it:
mkdir -p /tmp/shim
printf '#!/bin/sh\nexec python -m grpc_tools.protoc "$@"\n' > /tmp/shim/protoc
chmod +x /tmp/shim/protoc
PATH=/tmp/shim:$PATH uv run protol --create-package --in-place \
  --python-out src/common/generated protoc --proto-path=proto <proto set from the justfile _proto-fix recipe>
```

`proto/` and `src/common/generated/` are gitignored — this leaves `git status` clean (verified).

### 0.2 Checks — `libs/common` (most of the change)

```bash
cd libs/common
uv run pytest tests/ -q          # baseline: 42 passed
uv run ruff check . && uv run ruff format --check . && uv run ty check .
```

### 0.3 Checks — `apps/ws-ai-mcp`

`.env.example` **cannot be sourced** (`BASE_URL=https://<your-ngrok-host>/mcp_oauth` — the shell parses `<` as a redirect). Pass env inline:

```bash
cd apps/ws-ai-mcp
CX_ID=stg1 CORALOGIX_SCHEMA_STORE_ADDRESS=localhost:9090 OAUTH_CLIENT_ID=x \
OAUTH_CLIENT_SECRET=x BASE_URL=http://localhost:8080/mcp_oauth MCP_PORT=8080 \
REDIS_HOST=localhost REDIS_PORT=6379 REDIS_OAUTH_DB=0 JWT_SIGNING_KEY=test \
uv run pytest tests/ -q          # baseline: 13 passed
uv run ruff check . && uv run ty check .
```

### 0.4 Checks — `apps/api` (touched only by the step-1b refactor)

```bash
cd apps/api && uv run pytest tests/ut/test_urls.py tests/routes/test_slack.py -q && uv run ruff check . && uv run ty check .
```

### 0.5 CI

`.github/workflows/mcp_ci.yaml` (`just test` / `just smoke` for ws-ai-mcp); `.github/workflows/ci.yaml` (`common-ut`, `api-ut`, `lint` paths-filters). No new service directory ⇒ no CI registration needed per `.claude/rules/new-service-checklist.md`.

### 0.6 Before-state

`.saga/artifacts/mcp-write-tools-before.json` was produced by a **synthetic harness** driving the three impls with `AsyncMock` clients returning placeholder payloads — the payload *values* in it are illustrative only, not evidence of real API shapes. The load-bearing, code-verified observation is structural:

> `manage_dashboard_impl`, `manage_views_impl`, `manage_view_folders_impl`, `manage_dashboard_folders_impl` and `manage_cases_impl` all end with `return json.dumps(response, indent=2)`, where `response` is the **verbatim** `response.json()` from `CoralogixClient` (`create_dashboard:898`, `create_view`, `acknowledge_case`, … all `return response.json()` with no post-processing).

So no console link exists today and the response body is entirely upstream-controlled. Regenerate the after-artifact using the repo's own fixture shapes (`test_dashboards_kb_tools.py:22` → `{"id": "dashboard-1"}`, `test_cases_tools.py:52` → `{"ok": True}`), not invented ones.

---

## 1. Key findings that shape the design

### a) `resolve_frontend_team_url` is the right base, and the missing piece already exists in-repo

Today it hard-rejects non-header auth:

```python
if not isinstance(auth, CoralogixHeaderAuth):
    raise ValueError("auth_header is required to resolve the frontend team URL")
team_slug = await get_frontend_team_slug(auth.auth_header, settings.IDENTITY_SERVICE_URL)
return f"https://{team_slug}.app.{CORALOGIX_APP_ENV_TO_DOMAIN[settings.CORALOGIX_REGION]}"
```

The MCP builds `CoralogixApiKeyAuth` (`middleware/coralogix_clients_middleware.py:24`) — but `CoralogixApiKeyAuth.get_gateway_auth_header()` (`libs/common/src/common/auth/coralogix_auth.py`) **already** exchanges the bearer token at `GET {api_url}/api/v1/authctx/header` and returns an `x-coralogix-auth` value, which is exactly what `get_frontend_team_slug` consumes. The extension is: replace the `raise` with an api-key branch that mints the header and continues down the existing identity-gRPC path. No new endpoint, no new mechanism, no duplicated region map.

### b) The MCP frequently already has the raw header

`middleware/user_info_recorder.py:82` reads `headers.get("x-coralogix-auth")`, and the full header dict is in context state (`set_state("headers", ...)`). When present, build `CoralogixHeaderAuth` directly and skip the exchange.

### c) URL format: **plain paths, no `#/` prefix** — settled from cx-cli's git history

This was the defect in the previous attempt, and the ticket description's quoted strings (`.../#/dashboards/<id>`) are **stale**. Evidence, from the authoritative source rather than analogy:

1. **cx-cli PR #176's own commit history contains both the addition and the reversal.** An intermediate commit — `fix(console-url): add missing #/ hash-route prefix, add views console link` — added `#/` on the theory that the console is a hash-routed SPA. A later commit reverses it:

   > `fix(console-url): switch console links from hash routing to path routing`
   > "cx-web-workspace removed hash-based routing app-wide (the hash-routing-removal work) in favor of a custom HostedAppLocationStrategy that defaults to plain path routing. The only remaining hash carve-outs are hosted-app routes (/grafana, /opendashboards) and login routes reached via a legacy hash URL — none of which any cx console link builds toward. **Every generated console URL was still prefixing routes with `#/`, producing dead links.**"

   The ticket description quotes the *intermediate, since-reverted* state.
2. **The branch tip confirms it.** Verified live at `saga/forge-586-cli` head `daf6051`: every builder in `src/console_url.rs` is `format!("{}/dashboards/{id}", trim_base(base))`, `format!("{}/cases?id={encoded}", …)`, `format!("{}/explore?viewId={encoded}", …)`. The only three `#/` occurrences in the whole file are in the module doc explaining why the prefix is absent ("None of the builders below add a `#/` prefix"). Unit tests assert the exact plain-path output (`dashboard_url_joins_base_and_id`, `case_url_uses_query_param_shape`).
3. **Each route was cross-checked against the console frontend's routing source** (`coralogix/cx-web-workspace`), per a further commit *"cross-checked … directly against the console frontend's own routing source, not just public docs"*: `dashboardsEditUrl()` / `:id` route in `libs/dashboards/_ui/src/lib/routing-utils.ts`; `viewIdParam()` in `libs/explore/v2/src/lib/services/share-url.service.ts`; `SELECTED_CASE_QUERY_PARAM = 'id'` in `libs/cases/.../cases-query-params.constants.ts`. Also corroborated by public docs ("Share Dashboard URLs", "Deep links and URL parameters").
4. A separate commit notes the cx-dashboards skill's old hand-built `"https://<region>.app.coralogix.com/#/dashboards/<id>"` guidance was **"incorrect for several regions"** and was removed — the same lineage the ticket text inherits.
5. In-repo corroboration: `apps/api/src/api/utils/urls.py`'s `get_alert_base_url` and `get_skill_base_url` already bypass `create_frontend_link` for exactly this reason (docstring: *"the alert drilldown route is served outside the legacy `#/` hash-router"*).

**Decision:** build plain paths. To make this cheap to reverse if it is ever wrong, all three builders go through one `create_console_link` helper — a single-line change flips the whole feature. Confirm once in a browser before merge (§4.5b).

### d) Routes (cx-cli `src/console_url.rs` @ `daf6051`)

| Entity | Path |
|---|---|
| Dashboard | `{base}/dashboards/{id}` |
| Case | `{base}/cases?id={urlencoded id}` |
| View | `{base}/explore?viewId={urlencoded id}` |

### e) Upstream response shapes are not knowable from this repo — prefer request-side ids

Every client write method is `return response.json()` on a raw upstream call; the unit tests use arbitrary placeholders. So:

- **Cases:** id is always an input — `case_id = case_ids[0]` (`cases_tools.py:78`). cx-cli does the same (`cases.rs` passes `case_id` straight into `case_url`). **Zero response parsing.**
- **View update/delete:** the `view_id` argument.
- **Dashboard create/replace:** body contract is `{"requestId": "...", "dashboard": {...}}` (`tools_description_v2.py:207`), so `body["dashboard"]["id"]` is available request-side.
- **View create** is the only path that genuinely must parse the response.

Where parsing is needed, reuse cx-cli's empirically-validated probe chains (they run against the same backends and were verified live against a real team):
`dashboard_id_from_response` = `/dashboardId` → `/id` → `/dashboard/id` (`dashboards.rs:39`); `view_id_from_response` = `/view/id` → `/id` with numeric coercion (`views.rs:48`). cx-cli's own tests document that **both** view shapes occur in the wild (`view_id_from_response_reads_wrapped_shape`, `…_reads_bare_shape`: *"Some deployments return the created view directly, with no `view` envelope"*). Missing id ⇒ no field, never an error.

### f) Folders get no link

cx-cli has no folder URL builder, and its wiring commit states: *"cx dashboards folders create is intentionally left untouched — out of scope for this ticket."* `manage_dashboard_folders` create is in this ticket's success criteria but must be **skipped and reported in the PR** (the parent ticket's explicit skip-and-report escape hatch).

### g) The `*_impl` functions have exactly one consumer — the MCP wrappers

Grepped: no `apps/api` call sites. Adding an optional keyword-only parameter is safe and non-breaking.

---

## 2. Changes, in dependency order

### Step 1a — new `libs/common/src/common/utils/urls.py` (move + extend)

Move the two app-agnostic primitives out of `apps/api/src/api/utils/urls.py`, parameterised instead of reaching into `api.config.settings`:

```python
async def resolve_frontend_team_url(
    auth: CoralogixAuth,
    *,
    region: CoralogixRegion,
    identity_service_url: str,
    api_url: str,
    local_dev_url: str | None = None,
    call_timeout: float = 10.0,
) -> str:
    if local_dev_url is not None:
        return local_dev_url
    if isinstance(auth, CoralogixHeaderAuth):
        auth_header = auth.auth_header
    else:
        # NEW: api-key callers (MCP, Slack/Teams bots) exchange their bearer token
        # for a real x-coralogix-auth header via the existing gateway endpoint.
        headers = await auth.get_gateway_auth_header(api_url=api_url, request_timeout=call_timeout)
        auth_header = headers[X_CORALOGIX_AUTH_HEADER]
    team_slug = await get_frontend_team_slug(auth_header, identity_service_url, call_timeout)
    return f"https://{team_slug}.app.{CORALOGIX_APP_ENV_TO_DOMAIN[region]}"
```

`create_frontend_link` moves verbatim (still `#/`, still used by all existing `apps/api` callers). Add a sibling:

```python
def create_console_link(frontend_team_url: str, path: str) -> str:
    """Non-hash-routed console link (see §1c). Single choke point for the
    path-vs-hash decision — all entity builders go through here."""
    return f"{frontend_team_url.rstrip('/')}/{path.lstrip('/')}"
```

Reject a `#/`-prefixed `path` the way `create_frontend_link` rejects a doubled prefix.

### Step 1b — `apps/api/src/api/utils/urls.py` becomes a settings-bound adapter

Keep the module's public API identical so **no existing call site changes** (`slack_route`, `github_route`, `teams_route`, `scheduled_tasks_service`, `base_chat_bot_message_handler`, `scheduled_tasks_tools`, `servicenow_demo`, `thread_service`) and the patch targets in `apps/api/tests/routes/test_slack.py` keep working:

```python
async def resolve_frontend_team_url(auth: CoralogixAuth) -> str:
    return await common_resolve_frontend_team_url(
        auth,
        region=settings.CORALOGIX_REGION,
        identity_service_url=settings.IDENTITY_SERVICE_URL,
        api_url=get_coralogix_api_url(settings.CORALOGIX_REGION),
        local_dev_url=settings.FRONTEND_URL if (settings.IS_LOCAL_DEV or is_auth_test_mode()) else None,
    )
```

Everything else in that module (`ChatBaseUrls`, `get_chat_url`, `get_artifact_base_url`, `get_view_base_url`, `get_skill_base_url`, `get_alert_base_url`, `get_scheduled_task_*`, `resolve_chat_base_urls`) stays — chat/artifact-specific, no MCP consumer.

### Step 2 — new `libs/common/src/common/utils/console_url.py`

Pure, no I/O, no settings. Ports cx-cli's `console_url.rs` + `render.rs::tag_console_url`:

```python
def dashboard_url(base: str, dashboard_id: str) -> str:   # create_console_link(base, f"dashboards/{id}")
def case_url(base: str, case_id: str) -> str:             # create_console_link(base, f"cases?id={quote(id)}")
def view_url(base: str, view_id: str) -> str:             # create_console_link(base, f"explore?viewId={quote(id)}")

CONSOLE_URL_KEY = "consoleUrl"

def id_at(payload: object, *path: str) -> str | None:
    """Value at a key path, accepting str or int (ints stringified), else None.
    Ports cx-cli's json_str_at / view_id_from_response numeric coercion."""

def first_id(*candidates: str | None) -> str | None: ...

def tag_console_url(payload: object, url: str) -> object:
    """Port of cx-cli render.rs::tag_console_url. Decides at RUNTIME from the
    actual object — no assumption about upstream shape:
      - non-dict, or empty dict            -> unchanged
      - exactly one key whose value is a
        non-empty dict                     -> insert inside that nested dict
      - otherwise                          -> insert at the root
    (cx-cli's `_profile` carve-out is dropped — the MCP has no such key.)"""

ConsoleBaseResolver = Callable[[], Awaitable[str | None]]

async def resolve_and_tag(payload, resolver, build: Callable[[str], str | None]):
    """Best-effort: returns payload unchanged on ANY failure (no resolver,
    resolver returns None, no id, exception). Logs at debug. A console link
    must never fail a write."""
```

Adopting cx-cli's nesting rule verbatim keeps MCP and CLI payload shapes identical ("the same behavior needs to be copied"), and is safe because the rule inspects the object at runtime rather than assuming a shape.

### Step 3 — MCP settings

`apps/ws-ai-mcp/src/mcp_server/settings.py`:

```python
identity_service_url: str = Field(default="identity-service.aaa.svc.cluster.local.:9090", ...)
frontend_url: str | None = Field(default=None, description="Console base URL override (local dev / self-hosted)")
```

Default matches `apps/api/src/api/config.py:74`. Use `get_coralogix_api_url(coralogix_region_from_cx_id(settings.cx_id))` for the authctx-exchange `api_url`.

### Step 4 — `apps/ws-ai-mcp/src/mcp_server/console_url.py`

```python
async def resolve_console_base(ctx: Context) -> str | None:
    """Team console base URL for the current request. Never raises."""
```

1. `settings.frontend_url` set ⇒ return it.
2. Prefer `CoralogixHeaderAuth(auth_header=headers["x-coralogix-auth"], team_id=str(company_id))` from `await ctx.get_state("headers")`; else fall back to the request's existing `CoralogixApiKeyAuth` (`(await get_coralogix_client(ctx)).auth`), which triggers the exchange inside the shared resolver.
3. Call the shared `resolve_frontend_team_url(..., region=coralogix_region_from_cx_id(settings.cx_id), identity_service_url=settings.identity_service_url, call_timeout=5.0)`.
4. `cachetools.TTLCache` keyed on `company_id` (`cachetools`, `cachetools-async`, `async-cache` already dependencies). Positive TTL ~1 h; negative TTL ~60 s. Mirrors cx-cli's per-invocation `OnceCell` caching of `console_base()`.
5. Wrap in `try/except Exception` ⇒ log warning, return `None`.

### Step 5 — thread the resolver through the `*_impl` functions

Add keyword-only `console_base_resolver: ConsoleBaseResolver | None = None` (default `None` ⇒ today's behaviour) and apply `await resolve_and_tag(...)` immediately before the existing `return json.dumps(response, indent=2)`.

**`manage_cases_impl`** (`cases_tools.py:35`) — id is `case_ids[0]`, already computed at line 78; no response parsing. Tag the mutating single-case actions from the success criteria: `UPDATE`, `ASSIGN`, `UNASSIGN`, `ACKNOWLEDGE`, `UNACKNOWLEDGE`, `RESOLVE`, `CLOSE`, `SET_PRIORITY`, `CLEAR_PRIORITY`, plus `COMMENT` (cx-cli tags it too). Not `EVENTS` / `GET_EVENT` / `NOTIFICATIONS` (read-only; the latter two return before `case_id` exists).

**`manage_dashboard_impl`** (`dashboards_kb_tools.py:150`) — `CREATE` and `REPLACE` only. Id via
`first_id(id_at(body, "dashboard", "id"), id_at(response, "dashboardId"), id_at(response, "id"), id_at(response, "dashboard", "id"))`
— request-side first, then cx-cli's probe chain. Not `CHECK` (validation only) or `DELETE`.

**`manage_views_impl`** (`views_tools.py:132`) — `UPDATE`: use the `view_id` argument. `CREATE`: `first_id(id_at(response, "view", "id"), id_at(response, "id"))`, matching cx-cli's order and numeric coercion. Not `DELETE`.

**`manage_dashboard_folders_impl` / `manage_view_folders_impl`** — unchanged (§1f).

### Step 6 — MCP tool wrappers

`apps/ws-ai-mcp/src/mcp_server/tools/{dashboards/dashboards_kb_tools.py, views/views_tools.py, cases/cases_tools.py}` — pass `console_base_resolver=lambda: resolve_console_base(ctx)`. The lambda keeps resolution lazy so read-only actions never pay for it.

### Step 7 — tool descriptions

Update the affected entries in `apps/ws-ai-mcp/src/mcp_server/tools_description_v2.py` to state that successful mutations return a `consoleUrl` the model should surface, and that it may be absent.

*(AI-Center evaluation tools are deliberately excluded — not in the ticket's success criteria.)*

---

## 3. Tests

**`libs/common/tests/test_urls.py` (new)** — the auth extension is the risky part:
- header-auth path ⇒ `https://{slug}.app.{domain}`, incl. `FACTSET` / `PROOFPOINT` (patch `get_frontend_team_slug`);
- **api-key path** ⇒ assert `get_gateway_auth_header` is called *and* that the minted `x-coralogix-auth` value is what reaches `get_frontend_team_slug`;
- `local_dev_url` short-circuits with zero network calls;
- `create_frontend_link` still emits `#/`; `create_console_link` does not and rejects a `#/` path.

**`libs/common/tests/test_console_url.py` (new)** — port the relevant cases from cx-cli's `console_url.rs` / `render.rs` test modules:
- builders produce the exact plain-path strings (`.../dashboards/dash-abc123`, `.../cases?id=case-777`, `.../explore?viewId=…`), **explicitly asserting no `#/`**; base trailing-slash trimmed; ids containing `/`, spaces, `#` are URL-encoded;
- `id_at` / `first_id`: precedence, int coercion, all-missing ⇒ `None`;
- `tag_console_url`: nests inside a single non-empty object wrapper; stays at root for a flat multi-field object; stays at root for a single-key **string** value (`{"dashboardId": "dash-1"}`); stays at root for a single-key **array** value; no-op on non-dict, on `{}`, and on `{"wrapper": {}}`;
- `resolve_and_tag` returns the payload untouched when the resolver is `None`, returns `None`, or raises.

**Extend existing suites** — `test_dashboards_kb_tools.py`, `test_cases_tools.py`, `test_views_tools.py`, reusing each file's **existing** mock payload shapes:
- cases: every tagged action gets a `consoleUrl` derived from `case_ids[0]`, proven while keeping the existing `{"ok": True}` mock — i.e. the link works with a response carrying no id at all;
- dashboards: `REPLACE` with the existing `{"id": "dashboard-1"}` mock plus a body containing `dashboard.id` ⇒ request-side id wins; a create response with only `{"dashboardId": …}` ⇒ probe path; no id anywhere ⇒ **no** `consoleUrl` and response otherwise byte-identical;
- views: `UPDATE` uses the `view_id` argument even when the response is `{"ok": True}`; `CREATE` covers `{"view": {"id": …}}`, bare `{"id": …}`, numeric id, and the no-id case;
- folder and read-only actions unchanged;
- a resolver that raises leaves every response byte-identical to the current baseline.

**`apps/ws-ai-mcp/tests/test_console_url.py` (new)** — header-present path uses `CoralogixHeaderAuth` and performs no token exchange; header-absent path falls back to `CoralogixApiKeyAuth`; `settings.frontend_url` override wins; TTL cache keyed on `company_id` (two teams ⇒ two resolutions, same team twice ⇒ one); every failure mode returns `None`.

Mapping to `.claude/rules/verification.md`: exists (module/builders), substantive (real URLs), wired (field present end-to-end through the impls), functional (encoding, missing id, resolver failure, cache).

---

## 4. Verification

1. `libs/common`: `uv run pytest tests/ -q` (42 + new), ruff, `ty check`.
2. `apps/ws-ai-mcp`: `uv run pytest tests/ -q` with §0.3 env (13 + new), ruff, `ty check`.
3. `apps/api`: `uv run pytest tests/ut/test_urls.py tests/routes/test_slack.py -q` — proves step 1b kept the public API intact.
4. Regenerate `.saga/artifacts/mcp-write-tools-after.json` from the same harness using the repo's fixture shapes and a stub resolver returning `https://c4c.app.eu2.coralogix.com`; diff against the before file. Expected: `consoleUrl` added (at root or nested per §2 step 2) on each mutating response; folder / read / check / delete responses unchanged.
5. **Requires credentials / cluster access not available here** (do during implementation):
   a. confirm the ws-ai-mcp pod reaches `identity-service.aaa.svc.cluster.local:9090` (it already reaches `olly-kb-api.olly.svc.cluster.local:8080`, so same-cluster egress is expected — confirm before merge);
   b. open one produced URL of each kind in a real console to confirm the plain-path form loads the entity (§1c); if wrong, flip `create_console_link`;
   c. capture one real `create_view` response and check it against the probe order in step 5; add the captured shape as a fixture.

---

## 5. Risks & mitigations

| Risk | Mitigation |
|---|---|
| **Hash vs. path routing** — the ticket text says `#/`. | Resolved against the authoritative source (§1c): cx-cli added `#/`, then reverted it as *"producing dead links"*; branch tip `daf6051` has no `#/` in any builder; routes cross-checked against `cx-web-workspace` routing source. Single choke point (`create_console_link`) makes a reversal one line. Browser-confirmed pre-merge (§4.5b). |
| **Upstream response shape unknown** for the view-create probe. | Request-side ids used wherever they exist (cases, view update, dashboard create/replace); probe chains ported from cx-cli's live-verified implementation; a missed id degrades to "no `consoleUrl`", never an error; §4.5c captures the real shape. |
| **Identity service unreachable from the MCP's network** (insecure in-cluster gRPC; the MCP otherwise prefers public endpoints like `ng-api-grpc.{domain}:443`). | Verify first (§4.5a). If unreachable, the documented contingency — and only then — is to swap the slug lookup for `GET {api_url}/identity/whoami` → `team_url` (the public, key-readable endpoint cx-cli settled on), behind the same `resolve_frontend_team_url` signature so nothing downstream changes. Flag in the PR if used. |
| Extra network hop per write (identity gRPC, plus token exchange when the header is absent). | Lazy (mutating actions only), TTL-cached per team, 5 s timeout, all exceptions swallowed. |
| Wrong-team link from a shared cache. | Cache key is `company_id` from the request's own `user_info` state. |
| Step-1b refactor breaks an `apps/api` caller. | Public API of `api.utils.urls` unchanged; verified by `tests/ut/test_urls.py`, `tests/routes/test_slack.py`, `ty check`. |
| Additive contract change for MCP clients. | `consoleUrl` is purely additive; no existing field renamed or removed. |

---

## 6. Must be called out in the PR description (skipped / deviations)

- **Dashboard folders and view folders** — no per-entity console route exists; cx-cli explicitly left folder create untouched. `manage_dashboard_folders` create therefore ships **without** a link, a conscious deviation from this ticket's success criteria.
- `manage_dashboards` `check`, all `delete` actions, and the read-only case actions (`events`, `get_event`, `notifications`) — no single mutated entity to link to.
- Alerts and all read-only tools — out of scope per the ticket.
- **The ticket's `#/` URL format is stale** — link to the cx-cli reversal commit so reviewers don't re-raise it.
- Teams whose identity record has no frontend slug — resolution returns `None`, field simply absent.
- Whether the `/identity/whoami` contingency (§5) was needed, and any id-probe adjustments from §4.5c.
