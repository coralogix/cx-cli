---
name: rum
description: |
  Query and analyze Coralogix Real User Monitoring (RUM) data. Use this skill when the user asks about
  frontend errors, page load times, web vitals, user interactions, browser errors, mobile crashes,
  Core Web Vitals (LCP, CLS, FID, INP, TTFB), JavaScript exceptions, page performance, session errors,
  RUM data, real user monitoring, or any frontend/client-side observability question — even if they
  don't explicitly say "RUM".
version: 0.1.0
---

# RUM Querying Skill

Query and analyze Coralogix Real User Monitoring data using the `cx logs` command with DataPrime syntax.

## Understanding RUM in Coralogix

RUM captures real user interactions from browsers and mobile apps — errors, performance metrics, network requests, web vitals, and user interactions.

**RUM data is stored as regular logs.** All RUM events live in the `cx_rum` subsystem and are queried with the same `cx logs` command and DataPrime syntax used for any other logs. The difference is in the fields: all RUM-specific data lives under `$d.cx_rum.*`.

**What you can do:** Query, filter, aggregate, and analyze individual RUM log events — errors, page performance, user interactions, network requests, web vitals, mobile vitals.

**What you cannot do:** View RUM sessions as a complete unit (session replay), view session flows across pages, or access full session context. Only individual RUM log events are available.

For general log querying concepts and field discovery, see the **`query-logs`** skill. For DataPrime query language syntax, see the **[DataPrime Reference](../shared/references/dataprime-reference.md)**.

---

## CLI Command

RUM queries use `cx logs` — the same command as the `query-logs` skill:

```bash
cx logs '<dataprime_query>'
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | `now-1h` | Start time (ISO 8601 or relative, e.g. `now-7d`) |
| `--end` | `now` | End time |
| `--limit` | `100` | Maximum number of results |
| `--tier` | `frequent` | Storage tier: `frequent` or `archive` |
| `-o, --output` | `text` | Output format: `text`, `json`, or `agents` |

**RUM-specific default:** Use `--start now-7d` (or wider) for performance and web vitals queries. Short time ranges produce unreliable percentiles because low-traffic pages have too few data points.

---

## Identifying RUM Logs

**Every RUM query MUST include:**

```
filter $l.subsystemname == 'cx_rum'
```

### Application Filtering

For RUM data, use RUM-specific application fields — **NOT** `$l.applicationname`:

```bash
# Correct — RUM application name
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.version_metadata.app_name == "my-app"'

# Also correct — micro-frontend app label
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.labels.mfeApp == "my-app"'

# WRONG — $l.applicationname is not the RUM application name
cx logs 'filter $l.subsystemname == "cx_rum" && $l.applicationname == "my-app"'
```

---

## Event Types

Filter by `$d.cx_rum.event_context.type`:

| Type | Description |
|------|-------------|
| `error` | Errors, unhandled exceptions, crashes (browser and mobile) |
| `resources` | Resource loading (scripts, images, CSS, fonts) |
| `network-request` | XHR/Fetch HTTP requests |
| `user-interaction` | Clicks, inputs, scrolls |
| `web-vitals` | Web Vitals (LT, LCP, FID, CLS, FCP, INP, TTFB, TBT) |
| `longtask` | Long tasks blocking the main thread |
| `life-cycle` | Page lifecycle events (load, unload, visibility) |
| `dom` | DOM mutations and changes |
| `log` | Console logs captured by the SDK |
| `custom-measurement` | Custom metrics sent by the app |
| `mobile-vitals` | Mobile-specific performance metrics |

---

## Key Fields

All RUM fields live under `$d.cx_rum.*`. The fields you'll use most often:

| Context | Key Fields | Used For |
|---------|-----------|----------|
| `event_context` | `type`, `severity` (5 = error) | Filtering by event type and errors |
| `rum_template_id` | Error fingerprint | Grouping errors into distinct issues |
| `error_context` | `error_message`, `error_type`, `is_crash`, `original_stacktrace` | Error details |
| `session_context` | `user_id`, `session_id`, `browser`, `os`, `device`, `ip_geoip.*` | User/session identity |
| `version_metadata` | `app_name`, `app_version` | App filtering (use instead of `$l.applicationname`) |
| `page_context` | `page_url`, `page_fragments` (use for groupby) | Page identification |
| `network_request_context` | `url`, `fragments`, `method`, `status_code`, `duration` | HTTP request analysis |
| `web_vitals_context` | `name` (LT/LCP/FID/CLS/FCP/INP/TTFB/TBT), `value`, `rating` | Performance metrics |
| `interaction_context` | `target_element_inner_text` (use for groupby), `event_name` | Click/input analysis |
| `labels` | `mfeApp`, `mfeVersion` | Micro-frontend identification |

For the complete field reference including resource context, mobile contexts, and all sub-fields, see **[RUM Fields Reference](references/rum-fields.md)**.

---

## Querying RUM Data

### Errors and Issues

RUM errors can come from multiple event types: `error`, `network-request`, or `custom-log`. The universal identifiers are:

- **`event_context.severity == 5`** — indicates an error regardless of event type
- **`rum_template_id`** — groups similar errors into distinct issues

**Rules:**
- Always filter by severity: `$d.cx_rum.event_context.severity:num == 5`
- Always group by issue: `groupby $d.cx_rum.rum_template_id`
- Always filter null template IDs: `$d.cx_rum.rum_template_id != null`
- Always include descriptive fields with `any_value()` when aggregating
- Always include application name: `any_value($d.cx_rum.version_metadata.app_name) as app_name`

**Composing error descriptions:** When grouping by `rum_template_id`, include fields for all error types — irrelevant fields will be null:

- For `error` events → use `error_message`
- For `network-request` events → compose as `"<method> <url_fragments> (status <status_code>)"`
- For `custom-log` events → use `custom_log_message`

### Web Vitals

- Use **`percentile(0.75, ...)`** for p75 values — NOT `avg` (skewed by outliers)
- Use `$d.cx_rum.web_vitals_context.value` without `:num` cast
- **Only query the specific vitals the user asks about** — e.g., for "loading times" query only `LT`, for "LCP" query only `LCP`. Do NOT include all vitals unless explicitly asked for a full overview.
- For a single vital: `filter $d.cx_rum.web_vitals_context.name == 'LT'`
- For multiple vitals in one query: use conditional `if()` inside percentile
- **Use wide time ranges** (7+ days) for reliable percentiles

### User Interactions

- **NEVER return raw interaction events** — always aggregate
- **Always group by `interaction_context.target_element_inner_text`** — the button/link text the user sees
- **Always filter out empty/null inner text**
- Do NOT group by `target_element` (HTML tag) or `target_selector`
- Do NOT use `user_interaction_context` — the correct prefix is `interaction_context`

### Network Requests

- Filter by event type: `$d.cx_rum.event_context.type == 'network-request'`
- For errors: combine with `event_context.severity:num == 5`
- Compose description as: `"<method> <fragments> (status <status_code>)"`

### Page Performance

- Use the `LT` (Load Time) web vital for page loading time queries
- Group by `$d.cx_rum.page_context.page_fragments` (not `page_url`)
- Include user count for context: `distinct_count($d.cx_rum.session_context.user_id:string) as users`

---

## Common Query Patterns

### Top Errors by Issue

```bash
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.event_context.severity:num == 5 && $d.cx_rum.rum_template_id != null | groupby $d.cx_rum.rum_template_id aggregate count() as error_count, any_value($d.cx_rum.version_metadata.app_name) as app_name, any_value($d.cx_rum.event_context.type) as event_type, any_value($d.cx_rum.error_context.error_message) as error_message, any_value($d.cx_rum.network_request_context.method) as method, any_value($d.cx_rum.network_request_context.fragments) as url_fragments, any_value($d.cx_rum.network_request_context.status_code) as status_code, any_value($d.cx_rum.custom_log_context.message) as custom_log_message, distinct_count($d.cx_rum.session_context.user_id) as affected_users | orderby error_count desc' --start now-7d
```

### Network Request Errors

```bash
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.event_context.severity:num == 5 && $d.cx_rum.event_context.type == "network-request" | groupby $d.cx_rum.rum_template_id aggregate count() as error_count, any_value($d.cx_rum.version_metadata.app_name) as app_name, any_value($d.cx_rum.network_request_context.method) as method, any_value($d.cx_rum.network_request_context.fragments) as fragments, any_value($d.cx_rum.network_request_context.status_code) as status_code | orderby error_count desc' --start now-7d
```

### Slow Loading Pages (LT p75)

```bash
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.event_context.type == "web-vitals" && $d.cx_rum.web_vitals_context.name == "LT" | groupby $d.cx_rum.page_context.page_fragments aggregate distinct_count($d.cx_rum.session_context.user_id:string) as users, count() as page_views, percentile(0.75, $d.cx_rum.web_vitals_context.value) as LT_p75_ms | orderby users desc' --start now-7d
```

### User Interactions on a Page

```bash
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.event_context.type == "user-interaction" && $d.cx_rum.page_context.page_fragments ~ "/some/page" && $d.cx_rum.interaction_context.target_element_inner_text != null && $d.cx_rum.interaction_context.target_element_inner_text != "" | groupby $d.cx_rum.interaction_context.target_element_inner_text aggregate count() as click_count, distinct_count($d.cx_rum.session_context.user_id) as unique_users | orderby click_count desc' --start now-7d
```

### Affected Users per Error

```bash
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.event_context.severity:num == 5 && $d.cx_rum.rum_template_id != null | groupby $d.cx_rum.rum_template_id aggregate distinct_count($d.cx_rum.session_context.user_id) as affected_users, count() as error_count, any_value($d.cx_rum.error_context.error_message) as error_message | orderby affected_users desc' --start now-7d
```

### Single Web Vital (e.g., LCP by Page)

```bash
cx logs 'filter $l.subsystemname == "cx_rum" && $d.cx_rum.event_context.type == "web-vitals" && $d.cx_rum.web_vitals_context.name == "LCP" | groupby $d.cx_rum.page_context.page_fragments aggregate percentile(0.75, $d.cx_rum.web_vitals_context.value) as LCP_p75_ms, count() as samples | orderby LCP_p75_ms desc' --start now-7d
```

---

## Troubleshooting

If a query returns no results, change **one thing at a time**:

1. **Extend the time range**: `--start now-7d` or `--start now-30d`
2. **Relax filters**: remove the most restrictive condition
3. **Verify field names**: run a sample query with `-o json` to inspect actual fields
4. **Try archive tier**: `--tier archive --start now-30d` for older data

**Note:** Filtering by `cx_rum` fields will show **only RUM/frontend logs** and hide backend logs. This is expected behavior when analyzing RUM data.

---

## References

- **[RUM Fields Reference](references/rum-fields.md)** — Complete field reference for all RUM contexts (session, network, web vitals, mobile, etc.)
- **[`query-logs` skill](../query-logs/SKILL.md)** — General log querying, field discovery, investigation workflows, wildfind policy
- **[DataPrime Reference](../shared/references/dataprime-reference.md)** — Full query language: commands, operators, aggregations, text extraction, type conversions
- **[Logs Advanced Usage](../query-logs/references/advanced-usage.md)** — Investigation workflows, common query patterns, performance tips

For inline DataPrime help:

```bash
cx dataprime list                  # List all commands and functions
cx dataprime show filter           # Detailed help for a specific command
```

---

## Related Skills

- **`query-logs`** — General log querying with DataPrime (RUM data is logs)
- **`query-spans`** — Distributed traces and service latency
- **`metrics-query`** — Aggregated counters, gauges, and histograms (PromQL)
- **`telemetry-querying`** — Gateway skill for choosing the right data source
- **`cx-alerts`** — Create and manage alerts based on log patterns
