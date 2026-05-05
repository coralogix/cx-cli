# RUM Querying Reference

Query and analyze Coralogix Real User Monitoring data using the `cx logs` command with DataPrime syntax.

> **DataPrime syntax:** See `dataprime-reference.md` for the full query language reference.
> **Log querying basics:** See `logs-querying.md` for field discovery, wildfind policy, and general log query patterns.
> **Complete RUM field catalog:** See `rum-fields.md`.

## Understanding RUM in Coralogix

RUM captures real user interactions from browsers and mobile apps - errors, performance metrics, network requests, web vitals, and user interactions. **RUM data is stored as regular logs** in the `cx_rum` subsystem, queried with the same `cx logs` command and DataPrime syntax used for any other logs.

This means:
- **Metadata (`$m.*`)** and **labels (`$l.*`)** work the same as regular logs - you can filter on timestamp, severity, etc.
- **User data (`$d.cx_rum.*`)** contains all RUM-specific fields - event types, errors, sessions, web vitals, interactions, and more. See **[rum-fields.md](rum-fields.md)** for the complete field catalog.
- **Session replay and session flows are not available** - only individual RUM log events can be queried.

---

## CLI Command

```bash
cx logs '<dataprime_query>'
```

The `source logs` prefix is automatically injected if the query doesn't already include a `source` command.

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | `now-1h` | Start time (ISO 8601 or relative, e.g. `now-7d`) |
| `--end` | `now` | End time |
| `--limit` | `100` | Maximum number of results |
| `--tier` | `frequent` | Storage tier: `frequent` or `archive` |
| `-o, --output` | `text` | Output format: `text`, `json`, or `agents` |

**Note:** Use `--start now-7d` (or wider) for web vitals and page performance queries. Short time ranges produce unreliable percentiles - low-traffic pages have too few data points.

---

## RUM Data Model

### Identifying RUM Logs

Every RUM query must include `$l.subsystemname == 'cx_rum'`.

Application filtering in RUM uses dedicated fields - `$l.applicationname` does not map to the RUM application name:

```bash
# RUM application name
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.version_metadata.app_name == 'my-app'"

# Micro-frontend app label
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.labels.mfeApp == 'my-app'"

# WRONG - $l.applicationname is not the RUM application name
cx logs "filter \$l.subsystemname == 'cx_rum' && \$l.applicationname == 'my-app'"
```

### Event Types

Filter by `$d.cx_rum.event_context.type`:

| Type | Description |
|------|-------------|
| `error` | Errors, unhandled exceptions, crashes (browser and mobile) |
| `resources` | Resource loading (scripts, images, CSS, fonts) |
| `network-request` | XHR/Fetch HTTP requests |
| `user-interaction` | Clicks, inputs, scrolls |
| `web-vitals` | Web Vitals: `LT` (Load Time), `LCP`, `FID`, `CLS`, `FCP`, `INP`, `TTFB`, `TBT` |
| `longtask` | Long tasks blocking the main thread |
| `life-cycle` | Page lifecycle events (load, unload, visibility) |
| `dom` | DOM mutations and changes |
| `log` | Console logs captured by the SDK |
| `custom-measurement` | Custom metrics sent by the app |
| `mobile-vitals` | Mobile-specific performance metrics |

### Key Fields

All RUM fields live under `$d.cx_rum.*`. The most commonly used:

| Context | Key Fields | Used For |
|---------|-----------|----------|
| `event_context` | `type`, `severity` (5 = error) | Filtering by event type and errors |
| `rum_template_id` | Error fingerprint | Grouping errors into distinct issues |
| `error_context` | `error_message`, `error_type`, `is_crash`, `original_stacktrace` | Error details |
| `session_context` | `user_id`, `session_id`, `browser`, `os`, `device`, `ip_geoip.*` | User/session identity |
| `version_metadata` | `app_name`, `app_version` | App filtering (use instead of `$l.applicationname`) |
| `page_context` | `page_url`, `page_fragments` (use for groupby) | Page identification |
| `network_request_context` | `url`, `fragments`, `method`, `status_code`, `duration` | HTTP request analysis |
| `web_vitals_context` | `name`, `value`, `rating` | Performance metrics |
| `interaction_context` | `target_element_inner_text` (use for groupby), `event_name` | Click/input analysis |
| `labels` | `mfeApp`, `mfeVersion` | Micro-frontend identification |

### Error Detection

RUM errors can come from multiple event types (`error`, `network-request`, `custom-log`). The universal error marker is `event_context.severity == 5`, which applies regardless of event type.

The `rum_template_id` field groups similar error events into distinct issues - always group by it when analyzing errors, and filter out nulls:

```bash
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.severity:num == 5 && \$d.cx_rum.rum_template_id != null | groupby \$d.cx_rum.rum_template_id aggregate count() as error_count, any_value(\$d.cx_rum.version_metadata.app_name) as app_name, any_value(\$d.cx_rum.event_context.type) as event_type, any_value(\$d.cx_rum.error_context.error_message) as error_message, any_value(\$d.cx_rum.network_request_context.method) as method, any_value(\$d.cx_rum.network_request_context.fragments) as url_fragments, any_value(\$d.cx_rum.network_request_context.status_code) as status_code, any_value(\$d.cx_rum.custom_log_context.message) as custom_log_message, distinct_count(\$d.cx_rum.session_context.user_id) as affected_users | orderby error_count desc" --start now-7d
```

Include `any_value()` for descriptive fields from all error types - irrelevant fields will be null. When composing error descriptions from grouped results, the relevant fields depend on the event type:
- `error` → `error_message`
- `network-request` → `"<method> <url_fragments> (status <status_code>)"`
- `custom-log` → `custom_log_context.message`

---

## Essential Query Examples

```bash
# All RUM errors in the last 7 days
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.severity:num == 5" --start now-7d

# Network request errors
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.severity:num == 5 && \$d.cx_rum.event_context.type == 'network-request' | groupby \$d.cx_rum.rum_template_id aggregate count() as error_count, any_value(\$d.cx_rum.network_request_context.method) as method, any_value(\$d.cx_rum.network_request_context.fragments) as fragments, any_value(\$d.cx_rum.network_request_context.status_code) as status_code | orderby error_count desc" --start now-7d

# Slow loading pages (LT p75)
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.type == 'web-vitals' && \$d.cx_rum.web_vitals_context.name == 'LT' | groupby \$d.cx_rum.page_context.page_fragments aggregate distinct_count(\$d.cx_rum.session_context.user_id:string) as users, percentile(0.75, \$d.cx_rum.web_vitals_context.value) as LT_p75_ms | orderby users desc" --start now-7d

# User interactions on a page
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.type == 'user-interaction' && \$d.cx_rum.page_context.page_fragments ~ '/some/page' && \$d.cx_rum.interaction_context.target_element_inner_text != null && \$d.cx_rum.interaction_context.target_element_inner_text != '' | groupby \$d.cx_rum.interaction_context.target_element_inner_text aggregate count() as click_count, distinct_count(\$d.cx_rum.session_context.user_id) as unique_users | orderby click_count desc" --start now-7d

# Affected users per error
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.severity:num == 5 && \$d.cx_rum.rum_template_id != null | groupby \$d.cx_rum.rum_template_id aggregate distinct_count(\$d.cx_rum.session_context.user_id) as affected_users, count() as error_count, any_value(\$d.cx_rum.error_context.error_message) as error_message | orderby affected_users desc" --start now-7d

# LCP by page
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.type == 'web-vitals' && \$d.cx_rum.web_vitals_context.name == 'LCP' | groupby \$d.cx_rum.page_context.page_fragments aggregate percentile(0.75, \$d.cx_rum.web_vitals_context.value) as LCP_p75_ms, count() as samples | orderby LCP_p75_ms desc" --start now-7d
```

---

## Querying Patterns

### Web Vitals

Web vitals use `percentile(0.75, ...)` for p75 values - `avg` is skewed by outliers. Use `$d.cx_rum.web_vitals_context.value` without `:num` cast.

Only query the specific vitals the user asks about. For "loading times" query `LT`, for "LCP" query `LCP`. Include all vitals only when the user explicitly asks for a full overview.

For multiple vitals in one query, use conditional `if()` inside percentile:

```bash
cx logs "filter \$l.subsystemname == 'cx_rum' && \$d.cx_rum.event_context.type == 'web-vitals' | groupby \$d.cx_rum.page_context.page_fragments aggregate percentile(0.75, if(\$d.cx_rum.web_vitals_context.name == 'LT', \$d.cx_rum.web_vitals_context.value)) as LT_p75, percentile(0.75, if(\$d.cx_rum.web_vitals_context.name == 'LCP', \$d.cx_rum.web_vitals_context.value)) as LCP_p75" --start now-7d
```

### User Interactions

Always aggregate results - raw interaction events are noisy. Group by `interaction_context.target_element_inner_text` (the button/link text the user sees), and filter out null/empty values.

Do not group by `target_element` (HTML tag like DIV, SPAN) or `target_selector` - these are not meaningful to users. The correct field prefix is `interaction_context`, not `user_interaction_context`.

### Network Requests

Filter network requests by event type `$d.cx_rum.event_context.type == 'network-request'`. For failed requests, combine with `event_context.severity:num == 5`. Compose descriptions as `"<method> <fragments> (status <status_code>)"`.

### Page Performance

Use the `LT` (Load Time) web vital for page loading time questions. Group by `$d.cx_rum.page_context.page_fragments` (not `page_url`), and include user count for context with `distinct_count($d.cx_rum.session_context.user_id:string) as users`.

---

## Troubleshooting

If a query returns no results, change **one thing at a time**:

1. **Extend the time range**: `--start now-7d` or `--start now-30d`
2. **Relax filters**: remove the most restrictive condition
3. **Verify field names**: run a sample query with `-o json` to inspect actual fields
4. **Try archive tier**: `--tier archive --start now-30d` for older data

**Note:** Filtering by `cx_rum` fields will show **only RUM/frontend logs** and hide backend logs. This is expected when analyzing RUM data.
