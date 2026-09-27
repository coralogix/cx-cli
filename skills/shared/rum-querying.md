# RUM Querying Reference

Query and analyze Coralogix Real User Monitoring data using the `cx dataprime query` command against the `rum.events` dataset.

> **DataPrime syntax:** See `dataprime-reference.md` for the full query language reference.
> **Complete RUM field catalog:** See `rum-fields.md`.

## Understanding RUM in Coralogix

RUM captures real user interactions from browsers and mobile apps - errors, performance metrics, network requests, web vitals, and user interactions. **RUM data is stored in its own `rum.events` dataset**, not in logs. Query it with `source rum.events` and read the RUM payload flat under `$d.*`.

This means:
- **Always start the query with `source rum.events`** and run it with `cx dataprime query`. Do **not** use `cx logs` - it queries `source logs`, where RUM events do not live.
- **User data (`$d.*`)** contains all RUM-specific fields - event types, errors, sessions, web vitals, interactions, and more. Fields sit directly under `$d` (`$d.event_context.type`), with no `cx_rum` prefix. See **[rum-fields.md](rum-fields.md)** for the complete field catalog.
- **Do not filter by `$l.subsystemname`** - the `rum.events` dataset has no `cx_rum` subsystem label.
- **Never fall back to `source logs` or `cx_rum`.** If `source rum.events` returns no rows for the time range, report that no RUM data was found (optionally suggest widening the range); do not re-query logs.
- **Session replay and session flows are not available** - only individual RUM events can be queried.

---

## CLI Command

```bash
cx dataprime query 'source rum.events | <dataprime_query>'
```

Wrap the query in **single quotes** so the shell does not expand `$d`, and use double quotes for string literals inside it.

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | `now-1h` | Start time (ISO 8601 or relative, e.g. `now-7d`) |
| `--end` | `now` | End time |
| `--limit` | `100` | Maximum number of results |
| `--tier` | profile's `default_tier`, else `archive` | Storage tier: `frequent` or `archive` |
| `-o, --output` | `text` | Output format: `text`, `json`, or `toon` |

**Note:** Use `--start now-7d` (or wider) for web vitals and page performance queries. Short time ranges produce unreliable percentiles - low-traffic pages have too few data points.

---

## RUM Data Model

### Discovering Field Paths

Most fields are listed in `rum-fields.md`. `cx search-fields` does **not** cover RUM - it supports only logs and spans. For a field that is not listed:
- **You have a literal value** (e.g. an app name or label value the user quoted) - run `cx dataprime query 'source rum.events | wildfind "<literal>"' -o json`, then read the matching `$d.*` keypath off a returned record and use that path in the real query.
- **You only have a concept** (no example value) - sample with `cx dataprime query 'source rum.events | limit 10' -o json` and inspect a record for the right keypath.

Use `distinct $d.<path>` only *after* the path is known, to enumerate its values - it cannot discover a path you do not already have.

### Application Filtering

Application filtering in RUM uses dedicated fields - `$l.applicationname` does not map to the RUM application name:

```bash
# RUM application name
cx dataprime query 'source rum.events | filter $d.version_metadata.app_name == "my-app"'

# Micro-frontend app label
cx dataprime query 'source rum.events | filter $d.labels.mfeApp == "my-app"'

# WRONG - $l.applicationname is not the RUM application name
cx dataprime query 'source rum.events | filter $l.applicationname == "my-app"'
```

### Event Types

Filter by `$d.event_context.type`:

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

All RUM fields live under `$d.*`. The most commonly used:

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
cx dataprime query 'source rum.events | filter $d.event_context.severity:num == 5 && $d.rum_template_id != null | groupby $d.rum_template_id aggregate count() as error_count, any_value($d.version_metadata.app_name) as app_name, any_value($d.event_context.type) as event_type, any_value($d.error_context.error_message) as error_message, any_value($d.network_request_context.method) as method, any_value($d.network_request_context.fragments) as url_fragments, any_value($d.network_request_context.status_code) as status_code, any_value($d.custom_log_context.message) as custom_log_message, distinct_count($d.session_context.user_id) as affected_users | orderby error_count desc' --start now-7d
```

Include `any_value()` for descriptive fields from all error types - irrelevant fields will be null. When composing error descriptions from grouped results, the relevant fields depend on the event type:
- `error` → `error_message`
- `network-request` → `"<method> <url_fragments> (status <status_code>)"`
- `custom-log` → `custom_log_context.message`

---

## Essential Query Examples

```bash
# All RUM errors in the last 7 days
cx dataprime query 'source rum.events | filter $d.event_context.severity:num == 5' --start now-7d

# Errors per application
cx dataprime query 'source rum.events | filter $d.event_context.severity:num == 5 | groupby $d.version_metadata.app_name aggregate count() as error_count, distinct_count($d.rum_template_id) as distinct_issues, distinct_count($d.session_context.user_id) as affected_users | orderby error_count desc' --start now-7d

# Network request errors
cx dataprime query 'source rum.events | filter $d.event_context.severity:num == 5 && $d.event_context.type == "network-request" | groupby $d.rum_template_id aggregate count() as error_count, any_value($d.version_metadata.app_name) as app_name, any_value($d.network_request_context.method) as method, any_value($d.network_request_context.fragments) as fragments, any_value($d.network_request_context.status_code) as status_code | orderby error_count desc' --start now-7d

# Slow loading pages (LT p75)
cx dataprime query 'source rum.events | filter $d.event_context.type == "web-vitals" && $d.web_vitals_context.name == "LT" | groupby $d.page_context.page_fragments aggregate distinct_count($d.session_context.user_id:string) as users, percentile(0.75, $d.web_vitals_context.value) as LT_p75_ms | orderby users desc' --start now-7d

# User interactions on a page
cx dataprime query 'source rum.events | filter $d.event_context.type == "user-interaction" && $d.page_context.page_fragments ~ "/some/page" && $d.interaction_context.target_element_inner_text != null && $d.interaction_context.target_element_inner_text != "" | groupby $d.interaction_context.target_element_inner_text aggregate count() as click_count, distinct_count($d.session_context.user_id) as unique_users | orderby click_count desc' --start now-7d

# Affected users per error
cx dataprime query 'source rum.events | filter $d.event_context.severity:num == 5 && $d.rum_template_id != null | groupby $d.rum_template_id aggregate distinct_count($d.session_context.user_id) as affected_users, count() as error_count, any_value($d.error_context.error_message) as error_message | orderby affected_users desc' --start now-7d

# LCP by page
cx dataprime query 'source rum.events | filter $d.event_context.type == "web-vitals" && $d.web_vitals_context.name == "LCP" | groupby $d.page_context.page_fragments aggregate percentile(0.75, $d.web_vitals_context.value) as LCP_p75_ms, count() as samples | orderby LCP_p75_ms desc' --start now-7d
```

---

## Querying Patterns

### Web Vitals

Web vitals use `percentile(0.75, ...)` for p75 values - `avg` is skewed by outliers. Use `$d.web_vitals_context.value` without `:num` cast.

Only query the specific vitals the user asks about. For "loading times" query `LT`, for "LCP" query `LCP`. Include all vitals only when the user explicitly asks for a full overview.

For multiple vitals in one query, use conditional `if()` inside percentile:

```bash
cx dataprime query 'source rum.events | filter $d.event_context.type == "web-vitals" | groupby $d.page_context.page_fragments aggregate percentile(0.75, if($d.web_vitals_context.name == "LT", $d.web_vitals_context.value)) as LT_p75, percentile(0.75, if($d.web_vitals_context.name == "LCP", $d.web_vitals_context.value)) as LCP_p75' --start now-7d
```

### User Interactions

Always aggregate results - raw interaction events are noisy. Group by `interaction_context.target_element_inner_text` (the button/link text the user sees), and filter out null/empty values.

Do not group by `target_element` (HTML tag like DIV, SPAN) or `target_selector` - these are not meaningful to users. The correct field prefix is `interaction_context`, not `user_interaction_context`.

### Network Requests

Filter network requests by event type `$d.event_context.type == "network-request"`. For failed requests, combine with `event_context.severity:num == 5`. Compose descriptions as `"<method> <fragments> (status <status_code>)"`.

### Page Performance

Use the `LT` (Load Time) web vital for page loading time questions. Group by `$d.page_context.page_fragments` (not `page_url`), and include user count for context with `distinct_count($d.session_context.user_id:string) as users`.

---

## Troubleshooting

If a query returns no results, change **one thing at a time**. Keep the query window as **narrow** as possible and widen it deliberately — start from the window you already have rather than jumping to a huge range:

1. **Relax filters**: remove the most restrictive condition
2. **Verify field names**: run `cx dataprime query 'source rum.events | limit 10' -o json` to inspect actual fields
3. **Extend the time range**: widen gradually from your current window (e.g. `now-24h` → `now-7d` → `now-30d`)
4. **Try the other tier**: add `--tier frequent` or `--tier archive`, whichever the query did not use

If `source rum.events | limit 10` returns nothing across a wide window, the account has no RUM data for that period - report that instead of searching `source logs`.
