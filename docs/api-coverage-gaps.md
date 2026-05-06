# Coralogix v5 API Coverage Gaps

Analysis of Coralogix v5 REST APIs vs `cx` CLI commands.

## APIs fully or mostly covered by the CLI

| API Service | CLI Command | Notes |
|---|---|---|
| Alert Definitions | `alerts` | Covered (list/get/create/enable/disable). Missing: delete, replace, bulk-delete, bulk-replace, download |
| Alert Events | `alerts events/event-stats` | Covered |
| Alert Scheduler Rules | `alerts suppression-rules` | Covered |
| API Keys | `iam api-keys` | Covered |
| Connectors | `notifications connectors` | Covered |
| Custom Enrichments | `enrichments custom` | Covered |
| Dashboard Folders | `dashboards folders` | Missing: delete, get, replace |
| Dashboard Service | `dashboards` | Missing: delete, replace, assign-folder, favorites, get-by-slug, replace-default |
| Data Usage | `usage` | Covered (summary/daily/logs-count/spans-count/export-status) |
| Enrichments | `enrichments` | Covered |
| Events2Metrics | `e2m` | Covered |
| Extension Deployment | `integrations extensions` | Covered |
| Extension Service | `integrations extensions list/get` | Covered |
| Global Routers | `notifications routers` | Covered |
| Incidents | `incidents` | Covered |
| Outgoing Webhooks | `webhooks` | Covered |
| Policies (TCO) | `tco` | Covered |
| Presets | `notifications presets` | Covered |
| Recording Rules | `recording-rules` | Covered |
| Retentions | `retentions` | Covered |
| Role Management | `iam roles` | Covered |
| Rule Groups | `parsing-rules` | Covered |
| Scopes | `iam scopes` | Covered |
| SLOs | `slos` | Covered |
| Team Groups | `iam groups` | Covered |
| Views | `views` | Covered |
| View Folders | `views folders` | Covered |
| IP Access | `iam ip-access` | Covered |
| Contextual Data | `integrations contextual-data` | Covered |
| Actions (Webhooks) | `webhooks actions` | Covered |

## APIs / operations NOT available in the CLI

| API Service / Operation | Status | Notes |
|---|---|---|
| **Dashboard - delete** | Missing | DELETE endpoint exists but not wired |
| **Dashboard - replace** | Missing | PUT replace endpoint |
| **Dashboard - assign folder** | Missing | Move dashboard to folder |
| **Dashboard - favorites** | Missing | Add/remove from favorites |
| **Dashboard - get by slug** | Missing | Get by URL slug |
| **Dashboard - replace default** | Missing | Set default dashboard |
| **Dashboard Folders - delete** | Missing | DELETE folder |
| **Dashboard Folders - get** | Missing | GET single folder |
| **Dashboard Folders - replace** | Missing | PUT replace folder |
| **Alerts - delete** | Missing | DELETE alert definition |
| **Alerts - replace** | Missing | PUT replace alert |
| **Alerts - bulk delete** | Missing | Bulk delete alerts |
| **Alerts - bulk replace** | Missing | Bulk replace alerts |
| **Alerts - download** | Missing | Download all alerts |
| **Alerts - get by version ID** | Missing | Get alert by version |
| **Events Service** | **Not implemented** | Full service (list events, get event, batch get, stats, list counts) - separate from alert events |
| **Entities Service** | **Not implemented** | List entity types/subtypes |
| **Extension Testing Service** | **Not implemented** | Init/test/cleanup extension revisions |
| **Metrics Data Archive - validate** | Missing | Validate bucket endpoint |
| **Target Service** | **Not implemented** | Get/set target |
| **Team Config Service** | **Not implemented** | Team configuration management (6 operations) |
| **Integration - managed status** | Missing | Get managed integration status |
| **Integration - RUM versions** | Missing | RUM integration versions data |
| **Integration - sync RUM data** | Missing | Trigger sync of RUM data |
| **Integration - managed keys** | Missing | List managed integration keys |
| **Connectors - batch get summaries** | Missing | Batch get connector summaries |
| **Connectors - type summaries** | Missing | Get connector type summaries |

## Summary

- **Fully missing services (no CLI command at all):** Events Service, Entities Service, Extension Testing Service, Target Service, Team Config Service
- **Partially missing operations:** Dashboard (6 ops missing including delete), Alerts (5 ops missing including delete), Dashboard Folders (3 ops missing), Connectors (2 ops missing), Integrations (4 ops missing)
- **Well-covered:** ~30 of ~39 API services have good CLI coverage
