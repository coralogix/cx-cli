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
- **One milestone per fix:** Each endpoint fix is its own milestone with a full reproduce-fix-test-verify-commit cycle. This keeps git history clean (each commit maps to one specific Cloudflare error pattern), makes progress trackable, and lets us skip/reorder if one fix is blocked.
- **Retry with backoff for 429s:** Add to `CxClient` in `api_client.rs` rather than per-command - all commands benefit automatically.
- **Skill improvements over CLI changes:** Most 400 errors come from skills constructing bad payloads. Fix the skill guidance rather than adding client-side validation (the API already validates).
- **Better auth errors:** Improve `CxError::Auth` messages in `api_client.rs` to include the endpoint being called, so users can identify which permission they need.
- **Olly uses Clerk JWT auth:** The Olly API authenticates via Clerk JWT tokens with `cgx-team-id`, `cgx-user-id`, `cgx-user-name` headers - NOT Coralogix API keys. The cx-cli sends API keys as Bearer tokens which Olly rejects. This is a fundamental auth model mismatch that cannot be fixed with a simple path or permission change.
- **`latest` vs `5` version prefix:** The public API docs consistently use `/mgmt/openapi/5/` as the version prefix, but several cx-cli endpoints use `/mgmt/openapi/latest/`. The `latest` alias may route to an older API version (dec25) that lacks newer endpoints. All constants should be audited and aligned to version `5`.
- **gRPC colon syntax:** Some Coralogix REST endpoints use gRPC-style colon syntax for custom verbs (e.g., `/presets:summariesList`). Note: the grpc-gateway sometimes translates proto colon syntax to slash syntax in the public REST API (e.g., proto `logs:count` becomes REST `/logs/count`). Always verify against the public API docs at docs.coralogix.com, not just the proto definitions.
- **Cross-checked against public API docs:** All endpoint paths verified against `https://docs.coralogix.com/api-reference/v5/` on 2026-05-16. Proto definitions alone can be misleading due to grpc-gateway path transformations.

## Milestones Overview

**Code fixes (one per endpoint bug, reproduce-fix-verify):**
1. **Fix alerts events endpoint** - wrong service name `alerts-general` instead of `alerts` (41 errors/wk, 100%)
2. **Fix parsing-rules limits** - wrong HTTP method GET instead of POST (6 errors/wk, 100%)
3. **Fix preset summaries list** - wrong endpoint path `/summaries` instead of `:summariesList` (12 errors/wk, 100%)
4. **Fix double-slash metrics URL** - URL construction bug producing `//metrics/...` (7 errors/wk, 100%)
5. **Audit `latest` vs `5` version prefix** - cross-cutting prefix issue affecting connector test, possibly others (9+ errors/wk)

**DataPrime resilience (highest absolute impact):**
6. **Add 429 retry with backoff** - automatic retry for rate-limited requests (2,683 errors/wk)
7. **Improve DataPrime skill guidance** - reduce invalid query syntax from skills (3,485 errors/wk)

**Dashboard reliability:**
8. **Verify dashboard catalog fix** - confirm recent `/catalog/list` migration works (181 errors/wk)
9. **Improve dashboard creation skill** - fix JSON template quality (325 errors/wk)

**Data pipeline and notification skills:**
10. **Improve data pipeline skill** - fix enrichment rule and E2M payload quality (69 errors/wk)
11. **Improve observability-setup skill** - fix webhook, preset, and TCO payload quality (43 errors/wk)
12. **Improve incident management skill** - fix incident query payload quality (69 errors/wk)

**Investigations (root cause unknown):**
13. **Investigate data-usage count 400s** - path confirmed correct, unknown cause (10 errors/wk)
14. **Investigate "nonexistent" placeholder IDs** - source unknown, too frequent for test runs (330 errors/wk)
15. **Investigate extensions/deployed 404s** - path confirmed correct, possibly regional (7 errors/wk)

**Auth and permissions:**
16. **Document Olly auth incompatibility** - Clerk JWT vs API key mismatch (351 errors/wk)
17. **Improve auth error messages** - add endpoint context to 401/403 errors (1,200+ errors/wk)
18. **Investigate API keys list 401 pattern** - single user/region concentration (658 errors/wk)

---

## Milestone 1: Fix Alerts Events Endpoint

**Why this matters:** The alerts events endpoint returns 404 on every request (41 errors/wk, 100% failure rate). The CLI constructs the path under `alerts-general` but events live under a different service name `alerts`.

**Success criteria:** `cx alerts events list` returns valid event data instead of 404.

### Before/After

Currently `cx alerts events list` returns 404 because the path is `/alerts/alerts-general/v3/all/events`. After this milestone it uses `/alerts/alerts/v3/all/events` and returns valid data.

### 1.1 [ ] Fix alerts events service name
- **Files:** `src/commands/alerts/api.rs`, `src/commands/alerts/mod.rs`
- **What:** The `list_events()` method at `api.rs:235` constructs path `{ALERTS_BASE}/all/events` where `ALERTS_BASE` is `/mgmt/openapi/latest/alerts/alerts-general/v3`. This produces `/alerts/alerts-general/v3/all/events` which returns 404.
  - **Correct endpoint (from public API docs):** GET `/mgmt/openapi/5/alerts/alerts/v3/all/events` - the service name is `alerts/alerts` NOT `alerts/alerts-general`.
  - **Cross-reference:** Confirmed at `https://docs.coralogix.com/api-reference/v5/events-service/list-alert-events`
  - **Query params supported:** `alert_ids` (array), `timestamp_range` (from/to), `cx_event_labels`, `order_bys`, `pagination`
  - **Reproduce:** Run `cx alerts events list` and confirm 404 error.
  - **Fix:** Add a new constant `ALERT_EVENTS_BASE: &str = "/alerts/alerts/v3"` and change the events path to `format!("{ALERT_EVENTS_BASE}/all/events")`. Keep the existing `ALERTS_BASE` for CRUD operations on alert definitions (those work). Also verify the `alert-event-stats` endpoint at `api.rs:242`.
  - **Verify:** Re-run `cx alerts events list` and confirm it returns valid event data.
- **Acceptance:** Command returns valid data. Integration tests updated. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 2: Fix Parsing-Rules Limits

**Why this matters:** The parsing-rules limits endpoint returns 501 Not Implemented on every request (6 errors/wk, 100% failure rate). The CLI uses GET but the API requires POST.

**Success criteria:** `cx parsing-rules limits` returns valid usage limits data instead of 501.

### Before/After

Currently `cx parsing-rules limits` returns 501 because it sends a GET request. After this milestone it sends a POST and returns valid limits data.

### 2.1 [ ] Fix parsing-rules limits HTTP method
- **Files:** `src/commands/parsing_rules/api.rs`, `tests/parsing_rules/main.rs` (if exists)
- **What:** The `usage_limits()` method at `parsing_rules/api.rs:98` uses `self.client.get()` but the API requires **POST**.
  - **Confirmed by:** Public docs at `https://docs.coralogix.com/api-reference/v5/rule-groups-service/get-company-usage-limits` and proto at `rule_groups_service.proto:192`.
  - **Reproduce:** Run `cx parsing-rules limits` and confirm 501 Not Implemented.
  - **Fix:** Change `self.client.get(PARSING_RULES_LIMITS_BASE, &[])` to `self.client.post(PARSING_RULES_LIMITS_BASE, &json!({}))`.
  - **Verify:** Re-run and confirm valid usage limits data returned.
- **Acceptance:** Command returns valid data. Integration tests updated. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 3: Fix Preset Summaries List

**Why this matters:** The preset summaries list endpoint returns the default preset instead of a full list (12 errors/wk, 100% failure rate on the list operation). The CLI hits the wrong endpoint path.

**Success criteria:** `cx notifications presets list` returns all presets, not just the default.

### Before/After

Currently `cx notifications presets list` calls `/presets/summaries` which returns a single default preset. After this milestone it calls `/presets:summariesList` and returns all presets.

### 3.1 [ ] Fix preset summaries list endpoint path
- **Files:** `src/commands/presets/api.rs`, `tests/presets/main.rs` (if exists)
- **What:** The `list()` method at `presets/api.rs:73` calls GET `/presets/summaries` which is the endpoint for the **default** preset.
  - **Correct endpoint:** GET `/presets:summariesList` (confirmed at `https://docs.coralogix.com/api-reference/v5/presets-service/list-preset-summaries`). Supports optional query params: `connector_type`, `entity_type`.
  - **Reproduce:** Run `cx notifications presets list` and observe it returns a single preset or 400.
  - **Fix:** Change the list path from `format!("{PRESETS_BASE}/summaries")` to `format!("{PRESETS_BASE}:summariesList")`.
  - **Verify:** Re-run and confirm full list of presets returned.
- **Acceptance:** Command returns all presets. Integration tests updated. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 4: Fix Double-Slash Metrics URL

**Why this matters:** A URL construction bug produces `//metrics/...` paths that return 404 (7 errors/wk, 100% failure rate). Easy defensive fix in the HTTP client.

**Success criteria:** No double-slash URLs are possible regardless of endpoint config.

### Before/After

Currently, if `self.endpoint` has a trailing slash, the client produces `//metrics/api/v1/...` which returns 404. After this milestone, trailing slashes are stripped in the client constructor.

### 4.1 [ ] Fix URL construction to prevent double-slash
- **Files:** `src/api_client.rs`
- **What:** The client at `api_client.rs:41` uses `format!("{}{path}", self.endpoint)`. If `self.endpoint` has a trailing slash, this produces double-slash paths.
  - **Reproduce:** Check if any config or environment can produce a trailing-slash endpoint. Set up such a config and run `cx metrics search` to confirm 404.
  - **Fix:** Add `.trim_end_matches('/')` to `self.endpoint` in the `CxClient` constructor.
  - **Verify:** Confirm no double-slash URLs are produced. Run metrics commands successfully.
- **Acceptance:** No double-slash URLs possible. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 5: Audit `latest` vs `5` Version Prefix

**Why this matters:** The public API docs use `/mgmt/openapi/5/` but several cx-cli commands use `/mgmt/openapi/latest/` which may route to an older facade version (dec25) lacking newer endpoints. This is the likely cause of the connector test 404 (9 errors/wk, 100%) and may affect other endpoints.

**Success criteria:** All endpoint constants use version `5`. Commands that previously returned 404 due to version routing now work.

### Before/After

Currently, several commands use `/mgmt/openapi/latest/` which routes to an older API version. After this milestone, all use `/mgmt/openapi/5/` matching the public docs.

### 5.1 [ ] Audit and fix version prefix across all commands
- **Files:** All `src/commands/*/api.rs` files with endpoint base constants
- **What:** Grep for `"/mgmt/openapi/latest/"` across all api.rs files and change to `"/mgmt/openapi/5/"`.
  - **Reproduce:** Run `cx notifications test-connector <valid-connector-id>` and confirm 404.
  - **Fix:** Change all `latest` prefixes to `5`. Likely affected: `notification_testing/api.rs`, `alerts/api.rs`, `routers/api.rs`, `e2m/api.rs`, `integrations/api.rs`, `retentions/api.rs`.
  - **Verify:** Re-run connector test and other previously-failing commands. Confirm no more 404s from version routing.
- **Acceptance:** All constants use version `5`. `cargo fmt && cargo clippy && cargo test` pass. Connector test works.
- **Dependencies:** None

---

## Milestone 6: Add 429 Retry with Backoff

**Why this matters:** DataPrime queries get rate-limited 2,683 times/wk. Currently the CLI fails immediately on 429. Adding automatic retry transparently recovers from transient rate limits.

**Success criteria:** The CLI retries up to 3 times on 429 with exponential backoff. Users see fewer rate-limit failures.

### Before/After

Currently, 429 responses immediately fail with "Rate limited by the API." After this milestone, the CLI waits and retries transparently up to 3 times.

### 6.1 [ ] Implement retry logic in CxClient
- **Files:** `src/api_client.rs`
- **What:** Modify `CxClient` HTTP methods (`post`, `get`, `post_ndjson`, `get_ndjson`) to retry on 429:
  1. Read `Retry-After` header (already parsed at line 125-129)
  2. Wait `Retry-After` seconds (default 2s), then retry
  3. Exponential backoff: 1st retry = Retry-After, 2nd = 2x, 3rd = 4x
  4. Max 3 retries, then return 429 error as currently done
  5. Log to stderr on each retry: "Rate limited, retrying in {n}s..."
  - **Reproduce:** Run rapid concurrent `cx logs` calls until 429 is triggered. Confirm immediate failure.
  - **Verify:** After fix, re-run and confirm CLI retries transparently. Observe retry messages in stderr.
- **Acceptance:** Unit test for retry behavior. Manual verification. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 7: Improve DataPrime Skill Guidance

**Why this matters:** Skills generate 3,485 invalid DataPrime queries per week (400 errors). Better syntax guidance in the shared reference reduces these errors for all skills.

**Success criteria:** DataPrime reference includes common pitfalls. Measurable reduction in 400 rate over subsequent weeks.

### Before/After

Currently, the DataPrime reference lacks guidance on common syntax errors. After this milestone, a "Common Pitfalls" section covers the most frequent mistakes.

### 7.1 [ ] Add common pitfalls to DataPrime reference
- **Files:** `skills/shared/dataprime-reference.md`, `skills/cx-telemetry-querying/SKILL.md`
- **What:** Add a "Common Pitfalls" section covering:
  1. **Source prefix required:** Every query must start with `source logs` or `source spans`
  2. **Field path syntax:** `$d.field`, `$l.field`, `$m.field` - not `$field` or bare `field`
  3. **Type casting:** Use `:number`, `:string`, `:timestamp` - don't compare strings to numbers
  4. **String matching:** `~` for contains, `=~` for regex - not `LIKE` or `CONTAINS`
  5. **Aggregation in groupby:** Functions go inside `groupby`, not as separate pipe stages
  6. **Reserved words:** Escape with backticks
  7. **Timestamp handling:** Use `roundTime()`, not `date_trunc`
  Also update telemetry-querying skill to reference pitfalls section.
- **Acceptance:** Reference includes pitfalls section. `scripts/sync-shared-references.sh` run. All skill copies consistent.
- **Dependencies:** None

---

## Milestone 8: Verify Dashboard Catalog Fix

**Why this matters:** The dashboard catalog had 181 errors/wk (62.6% failure rate). Commit `9e97b28` changed the path from `/catalog` to `/catalog/list`. We need to verify it actually works.

**Success criteria:** `cx dashboards list` returns valid data.

### Before/After

Previously `cx dashboards list` returned 400. After commit `9e97b28` it should work. This milestone verifies that.

### 8.1 [ ] Verify catalog endpoint works
- **Files:** `src/commands/dashboards/api.rs`
- **What:** Confirm the fix:
  1. Verify `api.rs:166` uses `format!("{DASHBOARDS_BASE}/catalog/list")`
  2. Cross-reference with `openapi-facade-server/src/server/mod.rs:178` (route `/v1/dashboards`)
  - **Reproduce:** Run `cx dashboards list` and check if it succeeds or still returns 400.
  - **Verify:** If succeeds, mark as confirmed. If fails, investigate using `openapi-facade-server/schemas/logs-dashboards.yaml`, fix, and re-verify.
- **Acceptance:** `cx dashboards list` returns valid data. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 9: Improve Dashboard Creation Skill

**Why this matters:** Dashboard creation has 325 errors/wk (58.9% failure rate). The cx-create-dashboard skill generates invalid JSON payloads.

**Success criteria:** Skill templates are complete and validated against the canonical OpenAPI schema. Verification checklist catches common errors before deployment.

### Before/After

Currently, 58.9% of dashboard creation requests return 400 due to invalid payloads. After this milestone, the skill has validated templates, explicit query syntax rules, and a pre-deployment checklist.

### 9.1 [ ] Improve dashboard creation templates and verification
- **Files:** `skills/cx-create-dashboard/SKILL.md`, `skills/cx-create-dashboard/references/widget-templates.md`, `skills/cx-create-dashboard/references/query-syntax.md`, `skills/cx-create-dashboard/references/verification.md`
- **What:** Cross-reference with `openapi-facade-server/schemas/logs-dashboards.yaml` and improve:
  1. **widget-templates.md:** Complete, validated examples for all widget types with ALL required fields
  2. **query-syntax.md:** Rules for `queryType` matching, `source` prefix, `${__range}` variable, valid enum values
  3. **verification.md:** Pre-deployment checklist: valid definitions, no empty required arrays, variables defined, `requestId` present
  4. **SKILL.md:** Make Phase 6 (self-verify) more prescriptive
- **Acceptance:** Templates validated against OpenAPI schema. No missing required fields.
- **Dependencies:** None

---

## Milestone 10: Improve Data Pipeline Skill

**Why this matters:** Enrichment rule creation fails 56.9% of the time (37 errors/wk) and E2M fails 15.9% (11 errors/wk). The skill needs better payload guidance.

**Success criteria:** Skill includes validated examples and "template from existing" workflow.

### Before/After

Currently the skill constructs payloads from scratch that often fail validation. After this milestone it follows a "template from existing" pattern with validated examples.

### 10.1 [ ] Improve enrichment and E2M payload guidance
- **Files:** `skills/cx-data-pipeline/SKILL.md`
- **What:** Improve guidance:
  1. **Enrichment rules:** Add "template from existing" workflow - always `cx enrichments list -o json` first, modify existing JSON. Add required field checklist.
  2. **Custom enrichments:** Same pattern - template from existing.
  3. **E2M:** Add valid create payload examples with correct `e2m_type`, `metric_name`, `permutations`.
  4. **General:** Emphasize `--from-file` with validated JSON.
- **Acceptance:** Skill includes concrete validated examples. "Template from existing" workflow documented.
- **Dependencies:** None

---

## Milestone 11: Improve Observability-Setup Skill

**Why this matters:** Webhook creation fails 36.5% (31 errors/wk), notification preset updates fail 77.8% (7 errors/wk), and TCO reorder fails 55.6% (5 errors/wk).

**Success criteria:** Skill includes validated payload examples for webhooks, presets, and TCO operations.

### Before/After

Currently these operations frequently return 400 due to invalid payloads. After this milestone, the skill has validated examples and step-by-step guidance.

### 11.1 [ ] Improve webhook, preset, and TCO payload guidance
- **Files:** `skills/cx-observability-setup/SKILL.md`
- **What:**
  1. **Webhooks:** Add validated create payload examples with required fields. Document webhook types.
  2. **Presets:** Update to use corrected `:summariesList` endpoint from Milestone 3. Add validated update payload examples for `PUT /presets/custom`.
  3. **TCO reorder:** Document correct reorder payload format.
- **Acceptance:** Skill includes validated examples. All operations have step-by-step guidance.
- **Dependencies:** 3 (preset list fix)

---

## Milestone 12: Improve Incident Management Skill

**Why this matters:** The incidents endpoint has 69 errors/wk (25.8%) with mixed error codes: 400s from bad payloads, 500s from ap1 timeouts, 403s from permissions.

**Success criteria:** Skill includes validated query payloads and documents regional quirks.

### Before/After

Currently incident queries frequently fail with 400 (bad payloads) or 500 (ap1 timeouts). After this milestone, the skill has validated examples, pagination guidance, and regional notes.

### 12.1 [ ] Improve incident query payload guidance
- **Files:** `skills/cx-incident-management/SKILL.md`
- **What:**
  1. Add validated examples of incident list/filter payloads with correct field names
  2. Document that ap1 region may have higher latency for incident queries
  3. Add guidance on pagination to avoid large result sets triggering timeouts
  4. Document required permissions for incident operations
- **Acceptance:** Skill includes validated payload examples and regional guidance.
- **Dependencies:** None

---

## Milestone 13: Investigate Data-Usage Count 400s

**Why this matters:** The data-usage count endpoints return 400 (10 errors/wk, 100%) despite the path being confirmed correct by public API docs. Root cause is unknown.

**Success criteria:** Root cause identified and fixed, or documented as external dependency.

### Before/After

Currently `cx usage logs-count` and `cx usage spans-count` return 400 for unknown reasons. After this milestone, the root cause is known and either fixed or documented.

### 13.1 [ ] Investigate and fix data-usage count errors
- **Files:** `src/commands/data_usage/api.rs`, `src/commands/data_usage/mod.rs`
- **What:** Path confirmed correct at `https://docs.coralogix.com/api-reference/v5/data-usage-service/get-logs-count`.
  - **Reproduce:** Run `cx usage logs-count` and `cx usage spans-count`, capture exact error response body.
  - **Investigate:** Check if `Accept: text/event-stream` header is required (docs mention streaming). Check query parameter format. Compare with web app usage at `cx-web-workspace/apps/web-app/src/app/settings/shared/services/unified-data-usage.service.ts`.
  - **Fix:** Based on findings.
  - **Verify:** Re-run and confirm success.
- **Acceptance:** Root cause identified. Commands work or limitation documented.
- **Dependencies:** 5 (version prefix audit may affect this)

---

## Milestone 14: Investigate "nonexistent" Placeholder IDs

**Why this matters:** 330 DELETE requests/wk hit `/api-keys/v3/nonexistent` returning 401, plus 20/wk for `*/nonexistent-id-000`. All from `cx-cli/0.1.4` on `api.us1.coralogix.com`. Volume is too high for occasional test runs.

**Success criteria:** Source identified and eliminated.

### Before/After

Currently, ~350 errors/wk come from placeholder IDs being sent to production APIs. After this milestone, the source is identified and stopped.

### 14.1 [ ] Find and fix source of placeholder ID requests
- **Files:** `tests/write_command_gating/main.rs`, `tests/e2e/dashboards/mod.rs`, skill files
- **What:**
  1. **Check write_command_gating tests:** Do they use wiremock or hit real APIs? If real APIs, they run on every `cargo test` in CI, explaining the volume.
  2. **Check skills:** Search for patterns like `cx iam api-keys delete nonexistent` in skill files.
  3. **Check CI frequency:** If CI runs `cargo test` with a real API key on us1, that explains the concentration.
  - **Fix:** If tests: add wiremock mocking or move to `#[ignore]`. If skills: fix placeholder. If CI: isolate test key.
  - **Verify:** Monitor error counts to confirm requests stop.
- **Acceptance:** Source identified and eliminated.
- **Dependencies:** None

---

## Milestone 15: Investigate Extensions/Deployed 404s

**Why this matters:** GET `/integrations/extensions/v1/deployed` returns 404 for 50% of requests (7 errors/wk) despite the path being confirmed correct by public API docs.

**Success criteria:** Root cause identified and fixed, or documented as regional limitation.

### Before/After

Currently the command fails 50% of the time with 404. After this milestone, it works or the limitation is documented.

### 15.1 [ ] Investigate extensions/deployed 404s
- **Files:** `src/commands/integrations/api.rs`
- **What:** Path confirmed correct at `https://docs.coralogix.com/api-reference/v5/extension-deployment-service/get-deployed-extensions`.
  - **Reproduce:** Run the extensions deployed command, check if it returns 404.
  - **Investigate:** Check if the constant uses `latest` vs `5` prefix (may be fixed by M5). Check if 404s are region-specific. Compare with web app at `cx-web-workspace/libs/settings/extensions/src/lib/services/extensions-deployment.grpc.service.ts`.
  - **Fix:** Apply prefix fix or document regional limitation.
  - **Verify:** Re-run and confirm.
- **Acceptance:** Root cause identified. Fixed or documented.
- **Dependencies:** 5 (version prefix audit)

---

## Milestone 16: Document Olly Auth Incompatibility

**Why this matters:** Olly endpoints have 351 errors/wk (55-100% failure rate). The root cause is a fundamental auth model mismatch - Olly uses Clerk JWT, not API keys.

**Success criteria:** Olly limitation is documented. Users get a clear error message instead of opaque 403.

### Before/After

Currently `cx olly chat` returns an opaque 403 error. After this milestone, the error clearly explains that Olly requires user-session auth, and the skill documents the limitation.

### 16.1 [ ] Document Olly auth limitation and improve error message
- **Files:** `skills/cx-olly/SKILL.md`, `src/commands/olly/mod.rs`
- **What:** The Olly API uses Clerk JWT authentication. The web app sends `cgx-team-id`, `cgx-user-id`, `cgx-user-name` headers (source: `cx-web-workspace/libs/olly/src/lib/olly-auth.interceptor.ts`). The cx-cli sends API keys which Olly's `ClerkUserClaims` validation rejects (source: `olly/libs/common/src/common/auth/clerk_auth.py:115-150`).
  1. Update cx-olly skill to document the limitation
  2. Add a clear error message in `olly/mod.rs` when 403 is received
  3. Document medium-term options (gateway auth translation) and long-term (OAuth flow) for future work
- **Acceptance:** Skill documents the limitation. Error message is actionable. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 17: Improve Auth Error Messages

**Why this matters:** 1,200+ errors/wk are 401/403 across many endpoints. Users currently see generic "check API key scopes" with no indication of which endpoint failed. Better messages help users self-diagnose.

**Success criteria:** Auth errors include the endpoint path and HTTP method.

### Before/After

Currently 401/403 errors say "check API key scopes." After this milestone they say "401 Unauthorized on GET /mgmt/openapi/5/aaa/api-keys/v3/list - check that your API key has the required permission."

### 17.1 [ ] Add endpoint context to auth error messages
- **Files:** `src/api_client.rs`, `src/error.rs`
- **What:**
  1. In `checked_text()` (lines 134-141), include the endpoint path and HTTP method in 401/403 messages
  2. For 403, extract permission details from response body if available
  3. Update `CxError::Auth` in `error.rs` if needed
  - **Reproduce:** Run `cx iam api-keys list` with an underprivileged key. Observe generic error.
  - **Verify:** After fix, re-run and confirm error includes endpoint path and method.
- **Acceptance:** Auth errors include endpoint context. `cargo fmt && cargo clippy && cargo test` pass.
- **Dependencies:** None

---

## Milestone 18: Investigate API Keys List 401 Pattern

**Why this matters:** The API keys list endpoint has 90.8% failure rate (658 errors/wk), all from `cx-cli/0.1.4` on `api.us1.coralogix.com`. This single endpoint generates the second-highest error count.

**Success criteria:** Root cause understood and documented. cx-platform-admin skill updated with permission requirements.

### Before/After

Currently the API keys list endpoint fails 90.8% of the time with 401. After this milestone, the root cause is documented and the skill clearly states required permissions.

### 18.1 [ ] Investigate API keys list 401 concentration
- **Files:** `src/commands/api_keys/api.rs`, `skills/cx-platform-admin/SKILL.md`
- **What:**
  1. Check if this is a single user/profile with a bad key (concentration on us1 + single CLI version suggests this)
  2. Verify required permission for `/aaa/api-keys/v3/list` from proto definitions
  3. Update cx-platform-admin skill to document required key type/permission
  4. Consider adding a pre-check in skill before running IAM commands
- **Acceptance:** Root cause documented. Skill updated with permission requirements.
- **Dependencies:** 17 (better error messages help diagnosis)
