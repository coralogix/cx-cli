# CLI Failing Endpoints Investigation

**Linear project:** [CLI | Fix failing endpoints](https://linear.app/coralogix/project/cli-or-fix-failing-endpoints-ac6bc3bf2ce7/overview)
**Linear issue:** [AGE-889 - Investigate high failure rate of API endpoints](https://linear.app/coralogix/issue/AGE-889)
**Data window:** 7 days ending 2026-05-15
**Source:** Cloudflare HTTPRequests logs filtered by `cx-cli` User-Agent

---

## Overview

| # | Endpoint | Requests | Error % | Dominant Error | Category |
|---|----------|----------|---------|----------------|----------|
| 1 | `/api/v1/dataprime/query` | 69,022 | 9.4% | 400 (3,485), 429 (2,683), 403 (296) | Query |
| 2 | `/mgmt/openapi/5/aaa/api-keys/v3/list` | 725 | 90.8% | 401 (654) | Auth |
| 3 | `/mgmt/openapi/latest/alerts/alerts-general/v3` | 668 | 35.0% | 401 (184), 403 (25), 400 (23) | Auth + Bad Request |
| 4 | `/mgmt/openapi/5/dashboards/dashboards/v1` | 555 | 58.9% | 400 (325) on POST | Bad Request |
| 5 | `/api/v2/olly/v2/chats/` | 342 | 55.6% | 403 (190) | Permission |
| 6 | `/mgmt/openapi/5/aaa/api-keys/v3/nonexistent` | 330 | 100% | 401 (330) on DELETE | Wrong Path |
| 7 | `/mgmt/openapi/5/dashboards/dashboards/v1/catalog` | 289 | 62.6% | 400 (161) | Bad Request |
| 8 | `/mgmt/openapi/5/incidents/incidents/v1` | 267 | 25.8% | 400 (40), 403 (14), 500 (7) | Mixed |
| 9 | `/api/v2/olly/artifacts/` | 161 | 100% | 403 (161) | Permission |
| 10 | `/mgmt/openapi/5/dataplans/policies/v1` | 121 | 16.5% | 400 (8), 403 (8) | Mixed |
| 11 | `/mgmt/openapi/5/integrations/webhooks/v1` | 85 | 36.5% | 400 (29) on POST | Bad Request |
| 12 | `/mgmt/openapi/latest/events2metrics/events2metrics/v2` | 69 | 15.9% | 400 (6 POST, 3 PUT) | Bad Request |
| 13 | `/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1` | 65 | 56.9% | 400 (35) on POST | Bad Request |
| 14 | `/mgmt/openapi/5/dataplans/data-usage/v2` | 64 | 15.6% | 403 (6), 401 (3) | Auth |
| 15 | `/mgmt/openapi/latest/notifications/.../routers` | 64 | 26.6% | 403 (12), 401 (3) | Auth |
| 16 | `/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1` | 64 | 32.8% | 400 (16) on POST | Bad Request |
| 17 | `/mgmt/openapi/5/notifications/.../connectors` | 58 | 24.1% | 403 (6), 400 (3), 401 (3) | Mixed |
| 18 | `/mgmt/openapi/5/data-exploration/views/v1/views` | 49 | 14.3% | - | Auth |
| 19 | `/mgmt/openapi/5/dataplans/data-usage/v2/daily/processed-gbs` | 41 | 19.5% | 400 (6) on POST | Bad Request |
| 20 | `/mgmt/openapi/latest/alerts/.../v3/all/events` | 41 | 100% | 404 (41) | Wrong Path |
| 21 | `/mgmt/openapi/latest/integrations/integrations/v1` | 37 | 35.1% | 403 (10), 401 (3) | Auth |
| 22 | `/mgmt/openapi/5/actions/actions/v2` | 34 | 17.6% | - | Mixed |
| 23 | `/mgmt/openapi/5/slo/slos/v1` | 30 | 13.3% | - | Mixed |
| 24 | `/mgmt/openapi/5/aaa/team-groups/v2` | 29 | 17.2% | - | Auth |
| 25 | `/mgmt/openapi/5/aaa/custom-roles/v1` | 28 | 14.3% | - | Auth |
| 26 | `/mgmt/openapi/latest/dataengine/retention-tags/v1` | 26 | 34.6% | 403 (7), 401 (2) | Auth |
| 27 | `/mgmt/openapi/latest/dataengine/retention-tags/v1/enabled` | 21 | 28.6% | 403 (4), 401 (2) | Auth |
| 28 | `/mgmt/openapi/5/logs/data-setup/v2` | 21 | 33.3% | 403 (5), 401 (2) | Auth |
| 29 | `/api/v1/search-by-value` | 22 | 22.7% | 401 (4), 403 (1) | Auth |
| 30 | `/mgmt/openapi/5/aaa/team-saml/v1/configuration` | 17 | 100% | 403 (16) | Permission |
| 31 | `/mgmt/openapi/5/aaa/api-keys/v3` | 15 | 20.0% | - | Auth |
| 32 | `/mgmt/openapi/5/aaa/send-data-keys/v3` | 13 | 23.1% | - | Auth |
| 33 | `/mgmt/openapi/5/integrations/extensions/v1/deployed` | 14 | 50.0% | 404 (6) | Wrong Path |
| 34 | `/mgmt/openapi/5/notifications/.../presets/summaries` | 12 | 100% | 400 (10), 403 (2) | Bad Request |
| 35 | `/mgmt/openapi/5/dataplan/quota-rules/v1` | 12 | 75.0% | 403 (9) | Permission |
| 36 | `/mgmt/openapi/5/dataplans/data-usage/v2/logs/count` | 10 | 100% | 400 (9) | Bad Request |
| 37 | `/mgmt/openapi/5/dashboards/dashboards/v1/nonexistent-id-000` | 10 | 100% | 403 (9) on DELETE | Wrong Path |
| 38 | `/mgmt/openapi/5/dashboards/folders/v1/nonexistent-id-000` | 10 | 100% | 403 (9) on DELETE | Wrong Path |
| 39 | `/mgmt/openapi/5/notifications/.../presets/custom` | 9 | 77.8% | 400 (6) on PUT | Bad Request |
| 40 | `/mgmt/openapi/5/dataplans/policies/v1/all/reorder` | 9 | 55.6% | 400 (4) on POST | Bad Request |
| 41 | `/.../connectors/tests/config` | 9 | 100% | 404 (7) | Wrong Path |
| 42 | `//metrics/api/v1/label/__name__/values` | 7 | 100% | 404 (7) | CLI Bug |
| 43 | `/mgmt/openapi/5/parsing-rules/limits/v1` | 6 | 100% | 501 (6) | Not Implemented |

---

## Detailed Analysis by Error Category

### P0: High-volume endpoints with significant error rates

#### 1. DataPrime Query - `/api/v1/dataprime/query`
- **Volume:** 69,022 requests, 6,503 errors (9.4%)
- **Error breakdown:**
  - `400 Bad Request` (POST): **3,485** - malformed queries (syntax errors, invalid field references, etc.)
  - `429 Too Many Requests` (POST): **2,683** - rate limiting
  - `403 Forbidden` (POST): **296** - permission/auth issues
  - `500 Internal Server Error` (POST): **30** - server-side failures
  - `504 Gateway Timeout` (POST): 4
  - `499 Client Closed` (POST): 3
  - `520/503`: 2
- **Impact:** Highest absolute error count. The 429s suggest skills are sending too many concurrent queries. The 400s likely come from skills constructing invalid DataPrime syntax.
- **Action needed:**
  - Investigate rate-limiting patterns - are skills retrying properly on 429?
  - Audit skill-generated DataPrime queries for common syntax errors
  - Consider adding client-side rate limiting / backoff

#### 2. API Keys List - `/mgmt/openapi/5/aaa/api-keys/v3/list`
- **Volume:** 725 requests, 658 errors (90.8%)
- **Error breakdown:**
  - `401 Unauthorized` (GET): **654**
  - `403 Forbidden` (GET): 4
- **Sample details:** All errors from `cx-cli/0.1.4` on `api.us1.coralogix.com`. OriginResponseStatus is null.
- **Root cause hypothesis:** The API key being used likely lacks the required IAM permission (`team-api-keys-security-settings:ReadConfig` or similar). The endpoint requires elevated permissions that a standard "Send Your Data" key doesn't have.
- **Action needed:** Verify required permissions for this endpoint. The cx-platform-admin skill may be calling this with keys that lack IAM read access.

#### 3. Dashboards Create - `/mgmt/openapi/5/dashboards/dashboards/v1`
- **Volume:** 555 requests, 327 errors (58.9%)
- **Error breakdown:**
  - `400 Bad Request` (POST): **325** - dashboard creation failures
  - `403 Forbidden` (PUT/POST): 2
- **Regions affected:** us2, eu2, eu1 (all regions)
- **Root cause hypothesis:** The cx-create-dashboard skill is constructing invalid dashboard JSON payloads. This aligns with the Linear ticket note: "Hasuni's hypothesis is that the skill isn't building dashboard JSONs good enough."
- **Action needed:** Audit dashboard creation payloads for schema violations. Compare successful vs. failed requests to identify common payload issues.

#### 4. Dashboards Catalog - `/mgmt/openapi/5/dashboards/dashboards/v1/catalog`
- **Volume:** 289 requests, 181 errors (62.6%)
- **Error breakdown:**
  - `400 Bad Request` (GET): **161**
  - `401 Unauthorized` (GET): 15
  - `403 Forbidden` (GET): 5
- **Root cause hypothesis:** The catalog endpoint was recently updated (commit `9e97b28` - "Update dashboard catalog API endpoint to use /list suffix"). The 400s may be from the old path format still being used in some contexts, or the `/list` suffix not being appended correctly.
- **Action needed:** Verify that all code paths now use the `/catalog/list` endpoint after the recent migration.

#### 5. Alerts List - `/mgmt/openapi/latest/alerts/alerts-general/v3`
- **Volume:** 668 requests, 234 errors (35.0%)
- **Error breakdown:**
  - `401 Unauthorized` (GET): **184** - auth failures on list
  - `403 Forbidden` (GET): **25** - permission failures (concentrated on us2)
  - `400 Bad Request` (POST): **23** - alert creation failures
  - `500 Internal Server Error` (POST): 2
- **Action needed:** The 401/403 on GET likely stem from the same key permission issue as API keys. The 400s on POST need payload investigation.

#### 6. Alerts Events - `/mgmt/openapi/latest/alerts/alerts-general/v3/all/events`
- **Volume:** 41 requests, 41 errors (100%)
- **Error breakdown:**
  - `404 Not Found` (GET): **41**
- **Sample URIs:**
  - `/mgmt/openapi/latest/alerts/alerts-general/v3/all/events?start=now-24h`
  - `/mgmt/openapi/latest/alerts/alerts-general/v3/all/events?alert_id=<uuid>&start=...&end=...`
- **Root cause:** This endpoint path does not exist on the API. The `/all/events` suffix is wrong.
- **Action needed:** Fix the endpoint path in the CLI or skill. Check the alerts API documentation for the correct event-listing endpoint.

---

### P1: Medium-volume endpoints with high error rates

#### 7. Olly Chats - `/api/v2/olly/v2/chats/`
- **Volume:** 342 requests, 190 errors (55.6%)
- **Error breakdown:**
  - `403 Forbidden` (POST): **190** (100% of errors)
- **Regions affected:** eu1, eu2, us1
- **Root cause hypothesis:** The API key used by cx-cli likely lacks the Olly-specific permission. This is a gateway-level rejection (OriginResponseStatus is null).
- **Action needed:** Determine the required gateway permission for the Olly chat API and ensure cx-cli sends the correct authorization.

#### 8. Olly Artifacts - `/api/v2/olly/artifacts/`
- **Volume:** 161 requests, 161 errors (100%)
- **Error breakdown:**
  - `403 Forbidden` (GET): **161** (100%)
- **Regions affected:** Exclusively us1
- **Root cause:** Same permission issue as Olly chats. Every single request to this endpoint fails.
- **Action needed:** Same as Olly chats - fix the authorization/permission model.

#### 9. Incidents List - `/mgmt/openapi/5/incidents/incidents/v1`
- **Volume:** 267 requests, 69 errors (25.8%)
- **Error breakdown:**
  - `400 Bad Request` (POST): **40** - bad query payloads
  - `403 Forbidden` (POST): **14** - permission issues
  - `500 Internal Server Error` (POST): **7** - server failures (concentrated on ap1 with 21-22s origin response times)
  - `499 Client Closed` (POST): 3
  - `504 Gateway Timeout` (POST): 3
  - `401 Unauthorized` (POST): 2
- **Regions affected:** ap1 (500s with extreme latency), ap2 (400s), us2 (400s and 403s)
- **Action needed:**
  - Investigate 400s - likely malformed incident list/filter payloads
  - The 500s on ap1 with 21-22 second response times suggest a backend timeout - may need server-side investigation
  - The 403s suggest permission issues for certain profiles

#### 10. Enrichment Rules - `/mgmt/openapi/5/enrichment-rules/enrichment-rules/v1`
- **Volume:** 65 requests, 37 errors (56.9%)
- **Error breakdown:**
  - `400 Bad Request` (POST): **35** - bad creation payloads
  - `400 Bad Request` (PUT): 1
  - `401 Unauthorized` (GET): 1
- **Action needed:** Audit enrichment rule creation payloads for schema violations.

#### 11. Custom Enrichment Rules - `/mgmt/openapi/5/enrichment-rules/custom-enrichment-rules/v1`
- **Volume:** 64 requests, 21 errors (32.8%)
- **Error breakdown:**
  - `400 Bad Request` (POST): **16**
  - `401 Unauthorized` (GET): 2
  - `500 Internal Server Error` (POST): 1
  - `404 Not Found` (PUT): 1
  - `400 Bad Request` (PUT): 1
- **Action needed:** Similar to enrichment rules - audit payloads.

#### 12. Webhooks - `/mgmt/openapi/5/integrations/webhooks/v1`
- **Volume:** 85 requests, 31 errors (36.5%)
- **Error breakdown:**
  - `400 Bad Request` (POST): **29** - concentrated on 2026-05-14
  - `401 Unauthorized` (GET): 2
- **Action needed:** Investigate webhook creation payloads. The clustering on a single day suggests a specific skill session producing bad payloads.

---

### P2: Wrong path / Not Implemented endpoints

#### 13. API Keys Delete Nonexistent - `/mgmt/openapi/5/aaa/api-keys/v3/nonexistent`
- **Volume:** 330 requests, 330 errors (100%)
- **Error breakdown:**
  - `401 Unauthorized` (DELETE): **330**
- **Root cause:** The CLI or skill is constructing a DELETE request to a literal path `/nonexistent`. This is clearly a bug - likely a placeholder or default value that was never replaced with an actual key ID.
- **Action needed:** Find where in the CLI/skill code the string "nonexistent" is used as an API key ID and fix it.

#### 14. Dashboard/Folder Nonexistent IDs - `*/nonexistent-id-000`
- **Volume:** 10 each (dashboards + folders), 100% errors
- **Error breakdown:** 403 (9 each) on DELETE
- **Root cause:** Similar to API keys - the literal string `nonexistent-id-000` is being used as an ID. Likely a test/placeholder that's being sent in production.
- **Action needed:** Find and fix the source of `nonexistent-id-000` in the CLI/skill code.

#### 15. Extensions Deployed - `/mgmt/openapi/5/integrations/extensions/v1/deployed`
- **Volume:** 14 requests, 7 errors (50%)
- **Error breakdown:**
  - `404 Not Found` (GET): **6**
  - `401 Unauthorized` (GET): 1
- **Root cause:** The `/deployed` path may have been removed or renamed.
- **Action needed:** Verify the correct endpoint for listing deployed extensions.

#### 16. Notification Connector Tests - `/.../connectors/tests/config`
- **Volume:** 9 requests, 9 errors (100%)
- **Error breakdown:**
  - `404 Not Found` (POST): **7**
  - `401 Unauthorized` (POST): 2
- **Root cause:** Wrong endpoint path for testing notification connectors.
- **Action needed:** Verify the correct test-connector endpoint.

#### 17. Parsing Rules Limits - `/mgmt/openapi/5/parsing-rules/limits/v1`
- **Volume:** 6 requests, 6 errors (100%)
- **Error breakdown:**
  - `501 Not Implemented` (GET): **6**
- **Root cause:** This endpoint is not implemented on the server.
- **Action needed:** Remove or replace this API call in the CLI. Find alternative endpoint for parsing rule limits.

#### 18. Double-Slash Metrics - `//metrics/api/v1/label/__name__/values`
- **Volume:** 7 requests, 7 errors (100%)
- **Error breakdown:**
  - `404 Not Found` (GET): **7**
- **Root cause:** **CLI bug** - path construction produces a double leading slash `//metrics/...` instead of `/metrics/...`. Likely a string concatenation issue where the base URL already ends with `/` and the path also starts with `/`.
- **Action needed:** Fix the URL construction in `src/commands/metrics/api.rs` or wherever the metrics label values URL is built.

#### 19. Data Usage Logs/Count - `/mgmt/openapi/5/dataplans/data-usage/v2/logs/count`
- **Volume:** 10 requests, 10 errors (100%)
- **Error breakdown:**
  - `400 Bad Request` (GET): **9**
  - `403 Forbidden` (GET): 1
- **Root cause:** The `/logs/count` endpoint may expect different parameters or may have been deprecated.
- **Action needed:** Verify this endpoint still exists and what parameters it requires.

---

### P3: Auth/Permission errors (likely key permission issues)

These endpoints share a common pattern: 401/403 errors on GET (list) operations, suggesting the API keys used lack the required permissions for these management endpoints. These are likely caused by users running cx-cli with keys that don't have the necessary IAM permissions rather than CLI bugs.

| Endpoint | Errors | Status Codes |
|----------|--------|--------------|
| SAML Configuration | 17/17 (100%) | 403 (16), 401 (1) |
| Quota Rules | 9/12 (75%) | 403 (9) |
| Retention Tags | 9/26 (34.6%) | 403 (7), 401 (2) |
| Retention Tags /enabled | 6/21 (28.6%) | 403 (4), 401 (2) |
| Integrations List | 13/37 (35.1%) | 403 (10), 401 (3) |
| Notification Routers | 17/64 (26.6%) | 403 (12), 401 (3), 400 (1) |
| Notification Connectors | 14/58 (24.1%) | 403 (6), 401 (3), 400 (3) |
| Data Usage | 10/64 (15.6%) | 403 (6), 401 (3) |
| Logs Data Setup | 7/21 (33.3%) | 403 (5), 401 (2) |
| Search by Value | 5/22 (22.7%) | 401 (4), 403 (1) |
| Send Data Keys | 3/13 (23.1%) | auth errors |
| Team Groups | 5/29 (17.2%) | auth errors |
| Custom Roles | 4/28 (14.3%) | auth errors |

**Action needed:** Consider improving error messages when 401/403 is returned - tell the user which permission is required. Optionally, add a pre-flight permission check before calling these endpoints.

---

## Prioritized Fix List

Ranked by **impact** (errors/week, weighted by volume and error rate) and **effort** to fix.

Effort levels:
- **S** (Small) - clear bug fix or skill text change, < 1 hour
- **M** (Medium) - requires some investigation + code/skill changes, 1-4 hours
- **L** (Large) - root cause unclear, needs investigation before any fix, 4+ hours

### Tier 1: High impact, low effort (fix first)

| # | Issue | Errors/wk | Error % | Fix Type | Effort |
|---|-------|-----------|---------|----------|--------|
| 1 | **DataPrime 400s** - skills generating invalid DataPrime syntax | 3,485 | 5.0% of 69k | Skill improvement - audit common syntax errors in skill-generated queries, improve DataPrime reference in skills | **M** |
| 2 | **DataPrime 429s** - rate limiting from too many concurrent queries | 2,683 | 3.9% of 69k | Skill improvement - add guidance on query pacing; CLI improvement - add retry with backoff on 429 | **M** |
| 3 | **API Keys `/nonexistent`** - literal "nonexistent" used as key ID | 330 | 100% | Bug fix - find and remove the hardcoded "nonexistent" path in CLI or skill | **S** |
| 4 | **Dashboard create 400s** - invalid JSON payloads on POST | 325 | 58.6% of 555 | Skill improvement - improve cx-create-dashboard skill's JSON generation, add schema validation examples | **M** |
| 5 | **Alerts `/all/events` 404** - endpoint path doesn't exist | 41 | 100% | Bug fix - fix the event-listing endpoint path in CLI/skill | **S** |
| 6 | **Dashboard/folder `nonexistent-id-000`** - placeholder ID used in DELETE | 20 | 100% | Bug fix - find and remove hardcoded test ID | **S** |
| 7 | **Double-slash metrics `//metrics/...`** - path construction bug | 7 | 100% | Bug fix - fix URL join in metrics path construction | **S** |
| 8 | **Parsing rules limits 501** - endpoint not implemented | 6 | 100% | Bug fix - remove or replace this API call | **S** |

### Tier 2: High impact, medium effort (fix next)

| # | Issue | Errors/wk | Error % | Fix Type | Effort |
|---|-------|-----------|---------|----------|--------|
| 9 | **API Keys list 401s** - auth failures on list endpoint | 658 | 90.8% of 725 | Investigation needed - determine why keys lack permission; may need to change which key type is used or add permission check | **M** |
| 10 | **Dashboard catalog 400s** - GET returning 400 | 181 | 62.6% of 289 | Likely already fixed by commit `9e97b28` (added `/list` suffix) - verify the fix resolved it; if not, investigate further | **S-M** |
| 11 | **Olly chats 403s** - gateway permission rejection | 190 | 55.6% of 342 | Investigation needed - determine required gateway permission, may need platform-side change to allow cx-cli keys | **M** |
| 12 | **Olly artifacts 403s** - gateway permission rejection (100% failure) | 161 | 100% of 161 | Same root cause as Olly chats - fix together | **M** |
| 13 | **Alerts list 401/403** - auth failures on list GET | 209 | 31.3% of 668 | Investigation needed - same auth pattern as API keys; likely a key permission issue. The 23 POST 400s need separate payload investigation | **M** |

### Tier 3: Medium impact, variable effort

| # | Issue | Errors/wk | Error % | Fix Type | Effort |
|---|-------|-----------|---------|----------|--------|
| 14 | **Incidents 400/500s** - mixed errors, 500s on ap1 with 21s timeouts | 69 | 25.8% of 267 | 400s: skill improvement (bad filter payloads). 500s: server-side issue on ap1, needs backend team. 403s: permission issue | **M-L** |
| 15 | **Enrichment rules 400s** - bad creation payloads | 37 | 56.9% of 65 | Skill improvement - improve cx-data-pipeline skill's enrichment rule JSON generation | **S** |
| 16 | **Webhooks 400s** - bad POST payloads, clustered on one day | 31 | 36.5% of 85 | Skill improvement - improve cx-observability-setup skill's webhook creation guidance | **S** |
| 17 | **Custom enrichments 400s** - bad creation payloads | 21 | 32.8% of 64 | Skill improvement - same as enrichment rules, fix together | **S** |
| 18 | **TCO policies mixed errors** - 400s on create, 403s on list | 20 | 16.5% of 121 | 400s: skill improvement. 403s: permission issue | **S** |
| 19 | **Notification routers 403s** - permission failures on list | 17 | 26.6% of 64 | Permission issue - same root cause as other 403s | **M** |
| 20 | **Notification connectors mixed** - 403/400/401 | 14 | 24.1% of 58 | Mixed - 400s are skill improvement, 403/401 are permission | **S-M** |
| 21 | **Integrations list 403s** | 13 | 35.1% of 37 | Permission issue | **M** |
| 22 | **Notification presets/summaries 400s** | 12 | 100% of 12 | Bug fix or skill improvement - endpoint may require different parameters | **S** |
| 23 | **E2M 400s** - bad create/update payloads | 11 | 15.9% of 69 | Skill improvement - cx-data-pipeline skill | **S** |
| 24 | **Extensions deployed 404** - wrong endpoint path | 7 | 50% of 14 | Bug fix - verify correct endpoint path | **S** |
| 25 | **Data usage logs/count 400s** | 10 | 100% of 10 | Bug fix or investigation - endpoint may be deprecated | **S** |
| 26 | **Connector tests/config 404** - wrong path | 9 | 100% of 9 | Bug fix - fix endpoint path | **S** |
| 27 | **Notification presets/custom 400s** - bad PUT payloads | 7 | 77.8% of 9 | Skill improvement | **S** |
| 28 | **Quota rules 403s** | 9 | 75% of 12 | Permission issue | **M** |
| 29 | **TCO reorder 400s** | 5 | 55.6% of 9 | Skill improvement | **S** |

### Tier 4: Low impact or user-side (deprioritize)

These are primarily 401/403 errors on management endpoints that stem from users running cx-cli with keys that lack IAM permissions. Not CLI bugs - these are expected failures for underprivileged keys.

| Endpoint | Errors/wk | Notes |
|----------|-----------|-------|
| SAML Configuration | 17 | 403s - requires admin-level permissions, expected |
| Retention Tags | 15 | 403/401 - permission issue |
| DataPrime 403s | 296 | Permission issue - subset of users lack query access |
| Data Usage | 10 | 403/401 - permission issue |
| Logs Data Setup | 7 | 403/401 - permission issue |
| Search by Value | 5 | 401/403 - permission issue |
| Team Groups / Custom Roles / Send Data Keys | ~12 | Auth errors - permission issue |
| Views | 7 | Auth errors |

**Cross-cutting improvement:** Better error messages on 401/403 - tell the user which permission is missing. This is a single CLI-level improvement that helps all Tier 4 issues at once. **Effort: M**

---

## Recommended Execution Plan

**Sprint 1 - Quick wins (Tier 1 S-effort items):** Fix all hardcoded placeholder paths (`nonexistent`, `nonexistent-id-000`), double-slash metrics bug, alerts/events wrong path, parsing-rules/limits removal. ~6 small bug fixes, combined effort ~2-3 hours.

**Sprint 2 - Skill improvements (Tier 1 + 3 M-effort items):** Improve DataPrime query generation in skills (addresses 3,485 400s + 2,683 429s), improve dashboard creation skill (325 400s), improve enrichment/webhook/E2M/TCO skills (~100 combined 400s). These are all skill-text improvements, no CLI code changes needed.

**Sprint 3 - Auth/permission investigation (Tier 2):** Investigate the common 401/403 pattern across API keys, alerts, Olly, and management endpoints. Likely a single root cause (wrong key type or missing permission scope). Verify dashboard catalog fix from commit 9e97b28.

**Sprint 4 - Remaining items (Tier 3 investigations + Tier 4 cross-cutting):** Incidents ap1 500s (backend team), remaining small endpoint fixes, better 401/403 error messages.
