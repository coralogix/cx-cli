# Commands Status — M1–M3

Date: 2026-04-27  
Binary: installed from current branch (`cargo install --path .`)  
Profile: `default` (authenticated)

Read-only commands tested. Mutating subcommands (`create`, `update`, `delete`, `reorder`, `activate`) skipped.

---

## M1: SLOs, Recording Rules, Events2Metrics

| Command | Status | Notes |
|---------|--------|-------|
| `cx slos list` | ✅ OK | Returns list of SLOs |
| `cx slos get <ID>` | ✅ OK | Returns 400 for invalid GUID — expected behavior |
| `cx recording-rules list` | ✅ OK | Returns list of recording rule groups |
| `cx recording-rules get <ID>` | ✅ OK | Returns 404 for nonexistent ID — expected behavior |
| `cx e2m list` | ✅ OK | Returns list of E2M definitions |
| `cx e2m get <ID>` | ✅ OK | Returns 404 for nonexistent ID — expected behavior |
| `cx e2m labels-cardinality` | ✅ OK | Returns labels cardinality data |
| `cx e2m limits` | ✅ OK | Returns E2M limits |

**M1 summary: all commands working.**

---

## M2: TCO Policies, Data Usage, Retentions, Quota Rules

| Command | Status | Error | Reason |
|---------|--------|-------|--------|
| `cx tco-policies list` | ✅ OK | — | — |
| `cx tco-policies settings` | ❌ BROKEN | `400: validation failed … GetPolicy: "id" must be a valid GUID` | Implemented path `…/policies/v1/settings` appends `/settings` to the policies resource, which the server routes to `GetPolicy` expecting an ID. The correct endpoint is the separate service `/dataplans/policy-settings/v1` (GET). |
| `cx data-usage summary` | ❌ BROKEN | `400: Request time was not specified` | Path `/dataplans/data-usage/v2` is correct, but the server requires the `date_range` query param despite the OpenAPI spec marking it optional. The CLI sends no query params at all. |
| `cx data-usage daily` | ❌ BROKEN | `404: Not Found` | Implemented path: `…/data-usage/v2/daily/{data_type}` with `data_type` values `processed-gbs`, `units`, `eval-tokens`. Actual spec paths are fixed segments: `/dataplans/data-usage/v2/daily/processed-gbs`, `/…/daily/units`, `/…/daily/evaluation-tokens` (note: `evaluation-tokens` not `eval-tokens`). Dynamic `{data_type}` segment does not exist. |
| `cx data-usage logs-count` | ❌ BROKEN | `404: Not Found` | Implemented path: `…/data-usage/v2/logs-count`. Correct path: `/dataplans/data-usage/v2/logs/count` — uses a `/` separator, not a hyphen. |
| `cx data-usage spans-count` | ❌ BROKEN | `404: Not Found` | Implemented path: `…/data-usage/v2/spans-count`. Correct path: `/dataplans/data-usage/v2/spans/count` — same hyphen-vs-slash mistake as `logs-count`. |
| `cx data-usage export-status` | ✅ OK | — | — |
| `cx retentions list` | ❌ BROKEN | `404: Not Found` | Implemented path: `/dataplans/retentions/v1`. Correct path: `/dataengine/retention-tags/v1` — entirely different service (`dataengine`, not `dataplans`) and resource name (`retention-tags`, not `retentions`). |
| `cx retentions status` | ❌ BROKEN | `404: Not Found` | Implemented path: `…/retentions/v1/status`. Correct path: `/dataengine/retention-tags/v1/enabled` — wrong service namespace (same as above) and wrong sub-path (`/status` vs `/enabled`). |
| `cx quota-rules get` | ❌ BROKEN | `404: Not Found` | Implemented path: `/dataplans/quota-rules/v1`. Correct path: `/dataplan/quota-rules/v1` — service prefix is `dataplan` (singular), not `dataplans` (plural). One character difference. |

**M2 summary: 2 of 10 commands working. 8 broken.**

---

## M3: Connectors, Routers, Presets, Notification Test

| Command | Status | Error | Reason |
|---------|--------|-------|--------|
| `cx connectors list` | ❌ BROKEN | `404: Not Found` | Implemented path: `/notifications/connectors/v1`. Correct path: `/notifications/notification-center/v1/connectors` — missing the `notification-center` middle segment. All M3 notification services live under this sub-namespace. |
| `cx connectors types` | ❌ BROKEN | `404: Not Found` | Implemented path: `…/connectors/v1/types`. Correct path: `/notifications/notification-center/v1/connectors/types/summaries` — wrong base (missing `notification-center`) and wrong sub-path (`/types` vs `/types/summaries`). |
| `cx routers list` | ❌ BROKEN | `404: Not Found` | Implemented path: `/notifications/routers/v1`. Correct path: `/notifications/notification-center/v1/routers` — missing `notification-center` segment. |
| `cx presets list` | ❌ BROKEN | `404: Not Found` | Implemented path: `/notifications/presets/v1`. Correct path: `/notifications/notification-center/v1/presets` — missing `notification-center` segment. |
| `cx notification-test *` | N/A | — | All subcommands require `--from-file`; help renders correctly. Paths also use wrong base (`/notifications/notification-testing/v1`) — correct base is `/notifications/notification-center/v1`. |

**M3 summary: 0 of 4 testable commands working. All broken with 404.**

Root cause: All M3 paths omit the `notification-center` sub-namespace. The correct pattern is `/notifications/notification-center/v1/<resource>`, but the implementation used the guessed pattern `/notifications/<resource>/v1`.

---

## Overall Summary

| Milestone | Working | Broken | Total Tested |
|-----------|---------|--------|-------------|
| M1 (slos, recording-rules, e2m) | 8 | 0 | 8 |
| M2 (tco-policies, data-usage, retentions, quota-rules) | 2 | 8 | 10 |
| M3 (connectors, routers, presets, notification-test) | 0 | 4 | 4 |
| **Total** | **10** | **12** | **22** |

## Path Corrections Reference

| Command | Implemented Path | Correct Path (from openapi.yaml) |
|---------|-----------------|----------------------------------|
| `tco-policies settings` | `/dataplans/policies/v1/settings` | `/dataplans/policy-settings/v1` |
| `data-usage summary` | `/dataplans/data-usage/v2` ✓ | Same — but must pass `date_range` query param |
| `data-usage daily (processed-gbs)` | `/dataplans/data-usage/v2/daily/processed-gbs` ✓ | Same — but CLI type arg `eval-tokens` must become `evaluation-tokens` |
| `data-usage daily (dynamic)` | `/dataplans/data-usage/v2/daily/{data_type}` | Fixed paths: `/…/daily/processed-gbs`, `/…/daily/units`, `/…/daily/evaluation-tokens` |
| `data-usage logs-count` | `/dataplans/data-usage/v2/logs-count` | `/dataplans/data-usage/v2/logs/count` |
| `data-usage spans-count` | `/dataplans/data-usage/v2/spans-count` | `/dataplans/data-usage/v2/spans/count` |
| `retentions list` | `/dataplans/retentions/v1` | `/dataengine/retention-tags/v1` |
| `retentions status` | `/dataplans/retentions/v1/status` | `/dataengine/retention-tags/v1/enabled` |
| `quota-rules get` | `/dataplans/quota-rules/v1` | `/dataplan/quota-rules/v1` |
| `connectors list` | `/notifications/connectors/v1` | `/notifications/notification-center/v1/connectors` |
| `connectors types` | `/notifications/connectors/v1/types` | `/notifications/notification-center/v1/connectors/types/summaries` |
| `routers list` | `/notifications/routers/v1` | `/notifications/notification-center/v1/routers` |
| `presets list` | `/notifications/presets/v1` | `/notifications/notification-center/v1/presets` |
