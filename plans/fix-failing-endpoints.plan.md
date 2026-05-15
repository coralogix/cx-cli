# Plan: Fix Failing CLI Endpoints

| Field | Value |
|-------|-------|
| Status | draft |
| Created | 2026-05-16 |
| Ticket | AGE-889 |
| Branch | liranhason/age-889-investigate-high-failure-rate-of-api-endpoints |

## Context

The cx CLI has 43 endpoints with significant failure rates, totaling ~11,000 errors/week. Root causes fall into: wrong endpoint paths (100% failure, easy fixes), bad payloads from skills (400s), rate limiting (429s on DataPrime), and permission/auth issues (401/403). This plan addresses them in impact x effort order, starting with the easiest high-impact fixes.

See `plans/failing-endpoints-investigation.md` for the full data-driven investigation.

## Architecture Decisions

- **Reproduce-then-verify methodology:** For every bug fix, the workflow is: (1) reproduce the error with a concrete CLI command and confirm the failure, (2) apply the fix, (3) re-run the same command and confirm success. This ensures we have 100% confidence each fix resolves the issue.
- **Retry with backoff for 429s:** Add to `CxClient` in `api_client.rs` rather than per-command - all commands benefit automatically.
- **Skill improvements over CLI changes:** Most 400 errors come from skills constructing bad payloads. Fix the skill guidance rather than adding client-side validation (the API already validates).
- **Better auth errors:** Improve `CxError::Auth` messages in `api_client.rs` to include the endpoint being called, so users can identify which permission they need.
- **Olly uses Clerk JWT auth:** The Olly API authenticates via Clerk JWT tokens with `cgx-team-id`, `cgx-user-id`, `cgx-user-name` headers - NOT Coralogix API keys. The cx-cli sends API keys as Bearer tokens which Olly rejects. This is a fundamental auth model mismatch that cannot be fixed with a simple path or permission change.
- **`latest` vs `5` version prefix:** The public API docs consistently use `/mgmt/openapi/5/` as the version prefix, but several cx-cli endpoints use `/mgmt/openapi/latest/`. The `latest` alias may route to an older API version (dec25) that lacks newer endpoints. All constants should be audited and aligned to version `5`.
- **gRPC colon syntax:** Some Coralogix REST endpoints use gRPC-style colon syntax for custom verbs (e.g., `/presets:summariesList`). Note: the grpc-gateway sometimes translates proto colon syntax to slash syntax in the public REST API (e.g., proto `logs:count` becomes REST `/logs/count`). Always verify against the public API docs at docs.coralogix.com, not just the proto definitions.
- **Cross-checked against public API docs:** All endpoint paths verified against `https://docs.coralogix.com/api-reference/v5/` on 2026-05-16. Proto definitions alone can be misleading due to grpc-gateway path transformations.

## Milestones Overview

1. **Fix broken endpoint paths** - Eliminate 100%-failure-rate CLI bugs with known fixes (471 errors/wk, all S effort)
2. **Improve DataPrime resilience** - Add 429 retry logic and improve query guidance in skills (6,168 errors/wk)
3. **Improve dashboard creation reliability** - Fix skill payload quality and verify catalog endpoint (506 errors/wk)
4. **Improve data pipeline and notification skills** - Fix 400s in enrichments, webhooks, E2M, incidents, presets, TCO (200 errors/wk)
5. **Investigate remaining endpoint failures** - Data-usage counts, placeholder IDs, extensions deployed (347 errors/wk)
6. **Fix Olly and auth error handling** - Address permission rejections and improve error messages (1,058+ errors/wk)

---

## Milestone 1: Fix Broken Endpoint Paths

**Why this matters:** Multiple endpoints have 100% failure rates due to wrong paths, wrong HTTP methods, wrong version prefixes, or URL construction bugs. Every single request to these endpoints fails. These are the easiest wins - one-line fixes with known correct values confirmed from the public API docs.

**Success criteria:** All fixed endpoints return valid data. `cargo test` passes. Zero 404/501 errors from these paths in subsequent monitoring.

**Key decisions:** Correct paths confirmed from public API docs at `docs.coralogix.com/api-reference/v5/`, cross-referenced with `openapi-facade/may26/` proto definitions and `apidocs.swagger.json`. Each fix is atomic - one bug, one task, one commit.

### Before/After

Currently: alerts events returns 404 (wrong service name), parsing-rules limits returns 501 (wrong HTTP method), preset summaries list returns wrong data (wrong endpoint), a double-slash bug produces `//metrics/...`, and several endpoints using `latest` prefix route to older API versions missing newer endpoints. After this milestone, all paths match the public API docs.

### 1.1 [ ] Fix alerts events endpoint (wrong service name - 404)
- **Files:** `src/commands/alerts/api.rs`, `src/commands/alerts/mod.rs`
- **What:** The `list_events()` method at `api.rs:235` constructs path `{ALERTS_BASE}/all/events` where `ALERTS_BASE` is `/mgmt/openapi/latest/alerts/alerts-general/v3`. This produces `/alerts/alerts-general/v3/all/events` which returns 404.
  - **Correct endpoint (from public API docs):** GET `/mgmt/openapi/5/alerts/alerts/v3/all/events` - note the service name is `alerts/alerts` NOT `alerts/alerts-general`. The `/all/events` suffix is correct.
  - **Cross-reference:** Confirmed at `https://docs.coralogix.com/api-reference/v5/events-service/list-alert-events`
  - **Query params supported:** `alert_ids` (array), `timestamp_range` (from/to), `cx_event_labels`, `order_bys`, `pagination`
  - **Reproduce:** Run `cx alerts events list` and confirm it returns a 404 error.
  - **Fix:** Add a new constant `ALERT_EVENTS_BASE: &str = "/alerts/alerts/v3"` and change the events path to `format!("{ALERT_EVENTS_BASE}/all/events")`. Keep the existing `ALERTS_BASE` for CRUD operations on alert definitions (those work). Also verify the `alert-event-stats` endpoint at `api.rs:242` uses the correct base path.
  - **Verify:** Re-run the same command and confirm it now returns valid event data (not 404).
- **Acceptance:** `cx alerts events list` returns valid event data. Integration tests updated with correct paths and wiremock expectations. `cargo test` passes.
- **Dependencies:** None

### 1.2 [ ] Fix parsing-rules limits HTTP method (GET -> POST, returns 501)
- **Files:** `src/commands/parsing_rules/api.rs`, `tests/parsing_rules/main.rs` (if exists)
- **What:** The `usage_limits()` method at `parsing_rules/api.rs:98` uses `self.client.get()` but the API requires **POST**.
  - **Confirmed by:** Public docs at `https://docs.coralogix.com/api-reference/v5/rule-groups-service/get-company-usage-limits` and proto at `rule_groups_service.proto:192`.
  - **Reproduce:** Run `cx parsing-rules limits` and confirm it returns 501 Not Implemented.
  - **Fix:** Change `self.client.get(PARSING_RULES_LIMITS_BASE, &[])` to `self.client.post(PARSING_RULES_LIMITS_BASE, &json!({}))` (empty body POST).
  - **Verify:** Re-run and confirm it returns valid usage limits data.
- **Acceptance:** `cx parsing-rules limits` returns valid data. Integration tests updated. `cargo test` passes.
- **Dependencies:** None

### 1.3 [ ] Fix preset summaries list endpoint (wrong path, returns wrong data)
- **Files:** `src/commands/presets/api.rs`, `tests/presets/main.rs` (if exists)
- **What:** The `list()` method at `presets/api.rs:73` calls GET `/presets/summaries` which returns the **default** preset, not a list of all presets.
  - **Correct endpoint:** GET `/presets:summariesList` (confirmed by public docs at `https://docs.coralogix.com/api-reference/v5/presets-service/list-preset-summaries`). Supports optional query params: `connector_type`, `entity_type`.
  - **Reproduce:** Run `cx notifications presets list` and observe it returns a single preset (the default) instead of a full list, or returns 400.
  - **Fix:** Change the list path from `format!("{PRESETS_BASE}/summaries")` to `format!("{PRESETS_BASE}:summariesList")`.
  - **Verify:** Re-run and confirm it returns the full list of presets.
- **Acceptance:** `cx notifications presets list` returns all presets. Integration tests updated. `cargo test` passes.
- **Dependencies:** None

### 1.4 [ ] Fix double-slash metrics URL construction bug
- **Files:** `src/api_client.rs`
- **What:** The client at `api_client.rs:41` uses `format!("{}{path}", self.endpoint)`. If `self.endpoint` has a trailing slash, this produces `//metrics/...` (7 errors/wk, 100% failure).
  - **Reproduce:** Check if any config or environment can produce a trailing-slash endpoint. Set up such a config and run `cx metrics search` to confirm the 404.
  - **Fix:** Add `.trim_end_matches('/')` to `self.endpoint` in the `CxClient` constructor, protecting all endpoints at once.
  - **Verify:** Confirm no double-slash URLs are possible regardless of config input. Run metrics commands successfully.
- **Acceptance:** No double-slash URLs are possible. `cargo test` passes.
- **Dependencies:** None

### 1.5 [ ] Audit and fix `latest` vs `5` version prefix across all commands
- **Files:** All `src/commands/*/api.rs` files that define endpoint base constants
- **What:** The public API docs consistently use `/mgmt/openapi/5/` but several cx-cli commands use `/mgmt/openapi/latest/`. The `latest` prefix may route to an older facade version (dec25) that lacks newer endpoints. This is the likely root cause of the connector test 404 (9 errors/wk), and may contribute to errors on notifications, integrations, and other endpoints.
  - **Reproduce:** Run `cx notifications test-connector <valid-connector-id>` and confirm 404. Check `NOTIFICATION_TEST_BASE` and all other constants using `latest`.
  - **Fix:** Grep for `"/mgmt/openapi/latest/"` across all api.rs files. Change each occurrence to `"/mgmt/openapi/5/"` to match the public API docs. Affected files likely include: `notification_testing/api.rs`, `alerts/api.rs`, `routers/api.rs`, `e2m/api.rs`, `integrations/api.rs`, `retentions/api.rs`.
  - **Verify:** Re-run `cx notifications test-connector` and other previously-failing commands to confirm they no longer return 404.
- **Acceptance:** All endpoint constants use version `5`. No commands use `latest`. `cargo test` passes. Commands that previously returned 404 now work.
- **Dependencies:** None

### 1.6 [ ] Run tests and validate Milestone 1
- **Files:** None (validation only)
- **What:** Run the full test suite and verify all fixes:
  1. `cargo fmt && cargo clippy && cargo test`
  2. Verify fixed commands work: `cx alerts events list`, `cx parsing-rules limits`, `cx notifications presets list`, `cx notifications test-connector <id>`
  3. Verify `cx --help` and `cx schema` output are correct
  4. `cargo build --release` succeeds
- **Acceptance:** All checks pass.
- **Dependencies:** 1.1, 1.2, 1.3, 1.4, 1.5

---

## Milestone 2: Improve DataPrime Resilience

**Why this matters:** DataPrime is the CLI's most-used endpoint (69,022 requests/wk) and generates the most errors in absolute terms: 3,485 bad queries (400s) and 2,683 rate-limited requests (429s). Reducing these errors has outsized impact on overall CLI reliability.

**Success criteria:** The CLI automatically retries on 429 with exponential backoff (users see fewer rate-limit failures). Skills generate fewer invalid DataPrime queries (measurable reduction in 400 rate over subsequent weeks).

**Key decisions:** Retry logic goes in `CxClient` (not per-command) so all endpoints benefit. Max 3 retries with exponential backoff respecting the `Retry-After` header. Skills get improved DataPrime syntax guidance rather than client-side query validation (the server is the authority on valid syntax).

### Before/After

Currently, 429 responses immediately fail with an informational message. After this milestone, the CLI transparently retries up to 3 times with backoff. Currently, skills frequently generate invalid DataPrime queries. After this milestone, the shared DataPrime reference includes common pitfalls and the telemetry-querying skill has stricter guidance on query construction.

### 2.1 [ ] Add automatic retry with exponential backoff for 429 responses
- **Files:** `src/api_client.rs`
- **What:** Modify the `CxClient` HTTP methods (`post`, `get`, `post_ndjson`, `get_ndjson`) to automatically retry on 429 responses. Implementation:
  1. When a 429 is received, read the `Retry-After` header (already parsed at line 125-129)
  2. Wait for `Retry-After` seconds (or default 2s if not present), then retry
  3. Use exponential backoff: 1st retry waits Retry-After, 2nd waits 2x, 3rd waits 4x
  4. Max 3 retries, then return the 429 error as currently done
  5. Log a brief message to stderr on each retry (e.g., "Rate limited, retrying in {n}s...")
  6. The retry loop should wrap the existing `checked_text()` / `checked_ndjson()` call
  - **Reproduce:** Run many concurrent DataPrime queries (e.g., rapid `cx logs` calls in a loop) until a 429 is triggered. Confirm the current behavior: immediate failure with "Rate limited" message.
  - **Verify:** After implementing retry, re-run the same load test and confirm the CLI retries transparently and succeeds on subsequent attempts. Observe retry messages in stderr.
- **Acceptance:** Unit test demonstrating retry behavior. Manual test: run a query during rate limiting and observe retries in stderr. The existing 429 error message still appears after max retries exhausted.
- **Dependencies:** None

### 2.2 [ ] Improve DataPrime reference to reduce 400 errors
- **Files:** `skills/shared/dataprime-reference.md`, `skills/cx-telemetry-querying/SKILL.md`
- **What:** The 3,485 weekly 400 errors on DataPrime queries come from skills generating invalid syntax. Add a "Common Pitfalls" section to the shared DataPrime reference covering:
  1. **Source prefix required:** Every query must start with `source logs` or `source spans` - never omit it
  2. **Field path syntax:** `$d.field` for user data, `$l.field` for labels, `$m.field` for metadata - not `$field` or `field`
  3. **Type casting:** Use `:number`, `:string`, `:timestamp` for explicit casts - don't compare strings to numbers
  4. **String matching:** Use `~` for contains, `=~` for regex - not `LIKE` or `CONTAINS`
  5. **Aggregation in groupby:** Aggregation functions (`count()`, `avg()`, `sum()`) go inside the `groupby` command, not as separate pipe stages
  6. **Reserved words:** Escape field names that conflict with DataPrime keywords using backticks
  7. **Timestamp handling:** Use `roundTime()` for time bucketing, not date_trunc or similar SQL functions
  Also update the telemetry-querying skill to emphasize validating query syntax before sending.
- **Acceptance:** The dataprime-reference.md includes a clear "Common Pitfalls" section. The skill references it.
- **Dependencies:** None

### 2.3 [ ] Run tests and validate Milestone 2
- **Files:** None (validation only)
- **What:** Run full test suite:
  1. `cargo fmt && cargo clippy && cargo test`
  2. Verify the retry logic compiles and tests pass
  3. Run `scripts/sync-shared-references.sh` to distribute updated DataPrime reference to all consuming skills
  4. Verify skill files are consistent after sync
- **Acceptance:** All checks pass. Shared references are synced.
- **Dependencies:** 2.1, 2.2

---

## Milestone 3: Improve Dashboard Creation Reliability

**Why this matters:** Dashboard creation (`POST /dashboards/v1`) has 325 errors/wk (58.9% failure rate) - the 4th most-used endpoint is failing more than half the time. The dashboard catalog endpoint has 181 errors/wk (62.6%). Together these represent the most impactful skill-driven failures.

**Success criteria:** Dashboard creation success rate improves measurably. The catalog endpoint returns valid responses after the recent `/list` suffix migration.

**Key decisions:** Focus on improving the cx-create-dashboard skill's JSON generation guidance rather than adding client-side schema validation. The dashboard API is served by `openapi-facade-server` (Rust/axum) which uses hand-edited OpenAPI schemas - see `openapi-facade-server/schemas/logs-dashboards.yaml` for the canonical schema.

### Before/After

Currently, 58.9% of dashboard creation POST requests return 400 Bad Request because the skill generates invalid JSON payloads. After this milestone, the skill templates cover more edge cases, include explicit validation steps, and common structural errors are documented. The catalog endpoint (62.6% errors) is verified to work after the `/list` suffix migration.

### 3.1 [ ] Verify dashboard catalog endpoint fix
- **Files:** `src/commands/dashboards/api.rs`
- **What:** Commit `9e97b28` updated the catalog API endpoint from `/catalog` to `/catalog/list`. Verify this fix is correct:
  1. Confirm `api.rs:166` now uses `format!("{DASHBOARDS_BASE}/catalog/list")`
  2. The `openapi-facade-server` serves dashboards at route `/v1/dashboards` (GET) via `get_dashboard_catalog` handler (`openapi-facade-server/src/server/mod.rs:178`). The gateway maps this to the `/mgmt/openapi/5/dashboards/dashboards/v1/catalog/list` path used by cx-cli.
  - **Reproduce:** Run `cx dashboards list` and check if it still returns 400 or now succeeds after the recent fix.
  - **Verify:** If succeeds, the fix is confirmed. If still failing, compare with the `openapi-facade-server` schema at `schemas/logs-dashboards.yaml:23-29` and apply a further fix. Re-run and confirm success.
- **Acceptance:** `cx dashboards list` works without errors. Integration tests pass.
- **Dependencies:** None

### 3.2 [ ] Improve cx-create-dashboard skill JSON templates
- **Files:** `skills/cx-create-dashboard/SKILL.md`, `skills/cx-create-dashboard/references/widget-templates.md`, `skills/cx-create-dashboard/references/query-syntax.md`, `skills/cx-create-dashboard/references/verification.md`
- **What:** The skill generates dashboard JSON payloads that fail 58.9% of the time. The canonical schema is in `openapi-facade-server/schemas/logs-dashboards.yaml`. Improve the templates and guidance:
  1. **widget-templates.md:** Cross-reference with the OpenAPI schema to ensure all required fields are present. Add complete, validated examples for all widget types (gauge, pieChart, lineChart, dataTable, bar, markdown). The `openapi-facade-server` has manual patches (`.openapi-generator-ignore`) where REST diverges from gRPC - check for dashboard-specific divergences.
  2. **query-syntax.md:** Add explicit rules about:
     - `queryType` field must match widget type (instant vs time-series)
     - DataPrime queries in widgets MUST have `source logs` or `source spans` prefix
     - PromQL range queries must use `${__range}` variable
     - Color scheme and threshold configurations must use valid enum values
  3. **verification.md:** Add a pre-deployment checklist the skill must run through before calling `cx dashboards create`:
     - Every widget has a valid `definition` with `query` or `value`
     - No empty arrays for required fields
     - All referenced variables are defined in the dashboard's `variables` section
     - `requestId` is present and unique (16-byte hex from `new_request_id()`)
  4. **SKILL.md:** Make Phase 6 (self-verify structure) more prescriptive - list specific checks the agent must perform on the JSON before deploying
- **Acceptance:** The templates are complete and internally consistent. A manual review of the templates shows no missing required fields compared to the OpenAPI schema. The skill's verification phase is explicit enough to catch common errors.
- **Dependencies:** None

### 3.3 [ ] Run tests and validate Milestone 3
- **Files:** None (validation only)
- **What:** Run full test suite:
  1. `cargo fmt && cargo clippy && cargo test`
  2. Run `cx dashboards list` to verify catalog endpoint works
  3. Review the updated skill templates for completeness against `openapi-facade-server/schemas/logs-dashboards.yaml`
  4. Run `scripts/sync-shared-references.sh` if shared references were modified
- **Acceptance:** All checks pass. Dashboard list command works.
- **Dependencies:** 3.1, 3.2

---

## Milestone 4: Improve Data Pipeline and Notification Skills

**Why this matters:** Multiple data pipeline and notification endpoints have 30-57% error rates from skills sending bad creation payloads: enrichment rules (37 errors/wk, 56.9%), custom enrichments (21, 32.8%), webhooks (31, 36.5%), incidents (69, 25.8%), E2M (11, 15.9%), TCO reorder (5, 55.6%). These are all fixable through skill improvements.

**Success criteria:** Skills generate valid payloads for enrichment rules, webhooks, incidents, E2M, and TCO operations. Error rates decrease measurably.

**Key decisions:** Fix all data-pipeline skill issues together since they share the same skill file (cx-data-pipeline). Fix notification/webhook issues together since they share cx-observability-setup. Add incidents to the cx-incident-management skill.

### Before/After

Currently, the cx-data-pipeline skill generates enrichment rule and E2M payloads that fail ~40% of the time. The cx-observability-setup skill generates webhook and notification preset payloads that fail ~35-89% of the time. The cx-incident-management skill generates incident query payloads that fail 25.8% of the time (with 500s on ap1 region showing 21s timeouts). After this milestone, all skills include validated JSON templates and explicit payload construction guidance.

### 4.1 [ ] Improve cx-data-pipeline skill for enrichments and E2M
- **Files:** `skills/cx-data-pipeline/SKILL.md`
- **What:** The skill generates enrichment rule (56.9% error rate) and E2M (15.9% error rate) payloads that frequently return 400. Improve guidance:
  1. **Enrichment rules:** Add a "template from existing" workflow - always `cx enrichments list -o json` first, then modify an existing rule's JSON rather than constructing from scratch. Add required field checklist. Reference proto definitions from `openapi-facade/may26/proto/` for field names and types.
  2. **Custom enrichment rules:** Same pattern - template from existing, document required vs optional fields.
  3. **E2M definitions:** Add explicit examples of valid E2M create payloads with correct `e2m_type`, `metric_name` patterns, and `permutations` structure.
  4. **General:** Emphasize using `--from-file` with validated JSON rather than inline JSON in commands.
- **Acceptance:** The skill includes concrete, validated examples for enrichment rule and E2M creation. The "template from existing" workflow is clearly documented.
- **Dependencies:** None

### 4.2 [ ] Improve cx-observability-setup skill for webhooks and notifications
- **Files:** `skills/cx-observability-setup/SKILL.md`
- **What:** Multiple notification-related endpoints have high error rates:
  - Webhook creation: 29 POST 400s (36.5%)
  - Notification preset custom: 6 PUT 400s (77.8%)
  - TCO reorder: 5 POST 400s (55.6%)
  Improve the skill:
  1. **Webhooks:** Add validated create payload examples with required fields (name, type, url, config). Document the difference between webhook types. Reference `openapi-facade/may26/proto/` for the canonical field names.
  2. **Notification presets:** The preset list endpoint was fixed in Milestone 1 (task 1.3) to use `:summariesList`. Update the skill to reference the corrected command. For `PUT /presets/custom`, add validated update payload examples.
  3. **TCO reorder:** Add guidance on the correct reorder payload format (array of policy IDs in desired order).
- **Acceptance:** The skill includes validated examples for all failing operations. Webhook, preset, and TCO operations have clear step-by-step guidance.
- **Dependencies:** 1.3 (for preset summaries list fix)

### 4.3 [ ] Improve cx-incident-management skill for incidents queries
- **Files:** `skills/cx-incident-management/SKILL.md`
- **What:** The incidents endpoint has 69 errors/wk (25.8%) with mixed error codes:
  - 400 Bad Request (40): bad filter/query payloads, concentrated on ap2
  - 403 Forbidden (14): permission issues on us2
  - 500 Internal Server Error (7): server-side timeouts on ap1 (21-22s origin response times)
  - 499/504 (6): client/gateway timeouts
  Improve the skill:
  1. Add validated examples of incident list/filter payloads with correct field names and filter syntax
  2. Document that ap1 region may have higher latency for incident queries
  3. Add guidance on pagination to avoid large result sets that trigger timeouts
  4. Document required permissions for incident operations
- **Acceptance:** The skill includes validated payload examples. Common filter patterns are documented.
- **Dependencies:** None

### 4.4 [ ] Run tests and validate Milestone 4
- **Files:** None (validation only)
- **What:** Run full test suite:
  1. `cargo fmt && cargo clippy && cargo test`
  2. Review updated skill files for completeness and consistency
  3. Verify skill file formatting is correct (YAML frontmatter, markdown structure)
- **Acceptance:** All checks pass. Skill files are well-structured.
- **Dependencies:** 4.1, 4.2, 4.3

---

## Milestone 5: Investigate Remaining Endpoint Failures

**Why this matters:** Several endpoints have confirmed-correct paths but still fail. These require investigation to understand the root cause before applying a fix. Grouping them together avoids blocking the clear-fix milestones with investigation work.

**Success criteria:** Root cause identified for each endpoint. Fixes applied where possible, or issues documented as external/platform dependencies.

### Before/After

Currently, data-usage counts return 400 (correct path, unknown cause), "nonexistent" placeholder IDs generate 330 errors/wk from an unknown source, and extensions/deployed returns 404 on some regions. After this milestone, each root cause is understood and either fixed or documented with a clear next step.

### 5.1 [ ] Investigate data-usage count endpoint 400s
- **Files:** `src/commands/data_usage/api.rs`, `src/commands/data_usage/mod.rs`
- **What:** The paths `/dataplans/data-usage/v2/logs/count` and `/spans/count` are confirmed correct by the public API docs. The 400 errors (10/wk, 100%) must come from something else.
  - **Reproduce:** Run `cx usage logs-count` and `cx usage spans-count` and capture the exact error response (status code and body).
  - **Investigate:** The public docs mention the data-usage service returns data as "a stream of new-line delimited JSON objects" and may require `Accept: text/event-stream`. Check if cx-cli sends this header. Also check if query parameters are missing or malformed.
  - **Fix:** Based on the error response, add the required header or fix the request format.
  - **Verify:** Re-run both commands and confirm they return valid data.
- **Acceptance:** Root cause identified. Commands return valid data or issue documented as external dependency.
- **Dependencies:** 1.5 (version prefix audit may affect this)

### 5.2 [ ] Investigate "nonexistent" placeholder ID source (330 errors/wk)
- **Files:** `tests/write_command_gating/main.rs`, `tests/e2e/dashboards/mod.rs`, skill files
- **What:** 330 DELETE requests/wk hit `/mgmt/openapi/5/aaa/api-keys/v3/nonexistent` returning 401, and 20/wk hit `*/nonexistent-id-000` for dashboards/folders. All from `cx-cli/0.1.4` on `api.us1.coralogix.com`. This volume is too high for occasional E2E test runs (`#[ignore]`d tests). Investigate:
  1. **Check if write_command_gating tests hit real APIs:** These tests at `tests/write_command_gating/main.rs` use "nonexistent" as IDs. Do they use wiremock, or do they hit real Coralogix APIs? If they hit real APIs, they run on every `cargo test` invocation in CI.
  2. **Check if skills generate these:** Search skill files and Claude Code conversation logs for patterns like `cx iam api-keys delete nonexistent`. A skill might be using "nonexistent" as a placeholder.
  3. **Check CI frequency:** If CI runs `cargo test` frequently with a real API key, that would explain the volume from a single CLI version on a single region.
  - **Fix:** If tests: add wiremock mocking or move to `#[ignore]`d suite. If skills: fix the placeholder. If CI: isolate the test key.
  - **Verify:** Monitor error counts and confirm the "nonexistent" requests stop.
- **Acceptance:** Source of "nonexistent" requests identified and eliminated.
- **Dependencies:** None

### 5.3 [ ] Investigate extensions/deployed 404s
- **Files:** `src/commands/integrations/api.rs`
- **What:** GET `/integrations/extensions/v1/deployed` returns 404 for 6 of 14 requests (50%). The path is confirmed correct by public API docs at `https://docs.coralogix.com/api-reference/v5/extension-deployment-service/get-deployed-extensions`.
  - **Reproduce:** Run `cx integrations extensions deployed` (or equivalent) and check if it returns 404.
  - **Investigate:** Check if the constant uses `latest` vs `5` prefix (may be fixed by task 1.5). Check if 404s are region-specific (some regions may not have the extensions service deployed). Check the web app's gRPC call at `cx-web-workspace/libs/settings/extensions/src/lib/services/extensions-deployment.grpc.service.ts` for any special configuration.
  - **Fix:** Apply prefix fix if applicable, or document as regional limitation.
  - **Verify:** Re-run and confirm success (or document which regions lack this service).
- **Acceptance:** Root cause identified and fixed, or documented as regional limitation.
- **Dependencies:** 1.5 (version prefix audit)

### 5.4 [ ] Run tests and validate Milestone 5
- **Files:** None (validation only)
- **What:** Run full test suite:
  1. `cargo fmt && cargo clippy && cargo test`
  2. Verify all investigated endpoints work or have documented limitations
- **Acceptance:** All checks pass. All investigated issues resolved or documented.
- **Dependencies:** 5.1, 5.2, 5.3

---

## Milestone 6: Fix Olly and Auth Error Handling

**Why this matters:** Olly endpoints have 351 errors/wk (55-100% failure rate) from permission rejections. The API keys list endpoint has 658 errors/wk (90.8% failure rate). Beyond fixing specific permission issues, improving auth error messages across all 401/403 errors (1,200+ errors/wk) helps users self-diagnose and fix their key configuration.

**Success criteria:** Olly error is understood and documented (or fixed). All 401/403 errors across the CLI include actionable guidance about which permission is needed.

**Key decisions:** Olly uses Clerk JWT authentication with custom headers (`cgx-team-id`, `cgx-user-id`, `cgx-user-name`) - this is fundamentally incompatible with the cx-cli's API key auth model. The web app auth interceptor at `cx-web-workspace/libs/olly/src/lib/olly-auth.interceptor.ts` shows the required headers. Fixing this requires either platform-side changes or a new auth flow in cx-cli.

### Before/After

Currently, Olly chats/artifacts return opaque 403 errors. After this milestone, the Olly auth incompatibility is documented, the cx-olly skill warns users, and a platform ticket is filed if needed. Currently, 401/403 errors say "check API key scopes" generically. After this milestone, the error includes the endpoint path and HTTP method so users can identify exactly which permission is missing.

### 6.1 [ ] Document Olly auth incompatibility and update cx-olly skill
- **Files:** `skills/cx-olly/SKILL.md`, `src/commands/olly/mod.rs`
- **What:** The Olly API uses Clerk JWT authentication, not Coralogix API keys. The web app sends:
  - Standard Clerk JWT token (not a Coralogix API key)
  - `cgx-team-id` header (team context)
  - `cgx-user-id` header (user's teammate ID)
  - `cgx-user-name` header (user's email)
  (Source: `cx-web-workspace/libs/olly/src/lib/olly-auth.interceptor.ts`)
  
  The cx-cli sends a Coralogix API key as Bearer token, which Olly's `ClerkUserClaims` validation rejects (source: `olly/libs/common/src/common/auth/clerk_auth.py:115-150`).
  
  Options to evaluate:
  1. **Quick fix:** Update `cx-olly` skill to document this limitation. Add a clear error message in `olly/mod.rs` when 403 is received explaining that Olly requires user-session auth.
  2. **Medium-term:** Investigate if the Olly API can accept API keys via a gateway auth translation (similar to how other services accept API keys).
  3. **Long-term:** Add OAuth/Clerk auth flow to cx-cli for interactive use.
  
  For this milestone, implement option 1 and document options 2-3 for future work.
- **Acceptance:** The cx-olly skill clearly states the auth requirement. Running `cx olly chat` with an API key shows a helpful error message explaining why it fails.
- **Dependencies:** None

### 6.2 [ ] Improve auth error messages with endpoint context
- **Files:** `src/api_client.rs`, `src/error.rs`
- **What:** Currently, 401/403 errors produce generic messages like "check API key scopes". Improve them:
  1. In `api_client.rs` `checked_text()` (lines 134-141), include the endpoint path in the error message so users know which API call failed. E.g., "401 Unauthorized on GET /mgmt/openapi/5/aaa/api-keys/v3/list - run `cx profiles add` to update credentials, or check that your API key has the required permission."
  2. For 403 specifically, if the response body contains permission details (e.g., required scope), include that in the error message.
  3. Add the HTTP method (GET/POST/PUT/DELETE) to the error for better debugging context.
  4. Update `CxError::Auth` in `error.rs` if needed to carry additional context.
  - **Reproduce:** Run `cx iam api-keys list` with a key that lacks IAM permissions. Observe the current generic error message.
  - **Verify:** After the fix, re-run and confirm the error now includes the endpoint path and method.
- **Acceptance:** Running `cx iam api-keys list` with an underprivileged key shows an error that includes the endpoint path and method. Unit tests for error formatting pass.
- **Dependencies:** None

### 6.3 [ ] Investigate API keys list 401 pattern
- **Files:** `src/commands/api_keys/api.rs`, `skills/cx-platform-admin/SKILL.md`
- **What:** The API keys list endpoint has 90.8% 401 rate (658 errors/wk), all from cx-cli/0.1.4 on api.us1.coralogix.com. Investigate:
  1. Check if this is a single user/profile with a bad key hitting this repeatedly (the concentration on us1 and single CLI version suggests this)
  2. Verify what permission is needed for `/aaa/api-keys/v3/list` - check proto definitions in `openapi-facade/may26/proto/` for the ApiKeysService
  3. Update the cx-platform-admin skill to document the required key type/permission for IAM operations
  4. Consider adding a pre-check in the skill: before running IAM commands, verify the key has IAM permissions
- **Acceptance:** The cx-platform-admin skill clearly documents required permissions. The root cause of the 90.8% failure rate is understood and documented.
- **Dependencies:** 6.2

### 6.4 [ ] Run tests and validate Milestone 6
- **Files:** None (validation only)
- **What:** Run full test suite:
  1. `cargo fmt && cargo clippy && cargo test`
  2. Test error messages: run `cx iam api-keys list` or another privileged endpoint with a limited key and verify the error message is helpful
  3. Review updated skills for accuracy
- **Acceptance:** All checks pass. Error messages include endpoint context.
- **Dependencies:** 6.1, 6.2, 6.3
