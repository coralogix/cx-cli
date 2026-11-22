---
name: cx-product-analytics
description: >
  Analyze how users move through a web or mobile app with Coralogix Product
  Analytics and the `cx product-analytics` CLI — RUM applications, most visited
  pages, clicks, dead and rage clicks, custom events, funnels, conversion and
  drop-off, who dropped at a step, page paths, and heatmaps. Use when the user
  asks "where do users drop off", "funnel conversion", "conversion rate of
  checkout", "which users dropped at step 2", "what pages do users visit after
  /cart", "user journey", "user paths", "most clicked buttons", "rage clicks",
  "dead clicks", "heatmap", "top pages", "custom events", "create a funnel",
  "save a pathway", "is product analytics enabled", or wants product analytics
  on RUM data.
metadata:
  version: "0.1.0"
---

# Product Analytics Skill

Product Analytics turns RUM sessions into product questions: which pages users
visit, what they click, where they convert or drop off, and which paths they
take. Use `cx product-analytics` for these questions. For raw RUM events, errors
or web vitals, use `cx-telemetry-querying` (`rum.events`) instead.

## CLI Commands

| Command | Purpose |
|---|---|
| `cx product-analytics applications list` / `get <app>` | RUM applications with 30-day session and user counts |
| `cx product-analytics views list --app <app>` | Pages of an app, most visited first |
| `cx product-analytics events list --app <app> --view <view> --source auto\|custom` | Clicks (with dead and rage clicks) or custom events on a page |
| `cx product-analytics events get <eventId> --app <app> --view <view>` | One event's counts |
| `cx product-analytics funnels list\|get\|create\|update\|delete` | Saved funnels |
| `cx product-analytics funnels query [id]` | Conversion and drop-off per step, saved or inline |
| `cx product-analytics funnels matches [id] --step <s> --outcome converted\|dropped` | Users who reached or dropped at a step |
| `cx product-analytics pathways list\|get\|create\|update\|delete` | Saved pathways |
| `cx product-analytics pathways query [id] \| --app <app> --view <view>` | Most common page paths after or before a page |
| `cx product-analytics heatmaps list\|get\|create\|update\|delete` | Saved heatmaps |
| `cx product-analytics heatmaps query [id] \| --app <app> --view <view>` | Most clicked elements on a page (web only) |
| `cx product-analytics settings get` / `update --enabled true\|false` | Whether Product Analytics is on for the team |

- `create`, `update`, `delete` and `settings update` are writes: they need `--yes`
  in agent mode and are blocked by `--read-only`. Ask the user before running them.
- Every command supports `-o json`, `-o toon` and `-p <profile>` (repeatable).

## Time ranges

Commands that query data take `--start` / `--end` (default `now-1h` / `now`):
`now`, `now-7d`-style relative expressions, or ISO-8601 timestamps. The CLI sends
them as a fixed millisecond range.

When you run a saved funnel, pathway or heatmap by ID without `--start`/`--end`, no
time range is sent and the saved one applies. Ranges over 90 days work but print a
`costly` warning; prefer shorter ranges.

## Paging

`views list`, `events list`, `funnels matches` and the saved `list` commands return
one page. When there is more, stderr prints `more results: pass --page-token <token>`.
Repeat the same command with that `--page-token` within 15 minutes. `--page-size`
goes up to 500.

## Workflow

### 1. Check access and find the app

```bash
cx product-analytics settings get
cx product-analytics applications list
```

Every call fails with 412 when Product Analytics is off for the team. Tell the user;
only a team admin should turn it on (`settings update --enabled true --yes`). For
what users did meanwhile, fall back to `cx-telemetry-querying` (`rum.events`).

### 2. Explore pages and clicks

```bash
cx product-analytics views list --app web-shop --start now-7d
cx product-analytics events list --app web-shop --view /cart --source auto --start now-7d
cx product-analytics events list --app web-shop --view /cart --source custom --start now-7d
```

Pass `view` exactly as `views list` prints it. Clicks carry dead-click and rage-click
percentages, a good signal of UX problems.

### 3. Funnels

```bash
cx product-analytics funnels list --app web-shop
cx product-analytics funnels query 42 --start now-30d
cx product-analytics funnels matches 42 --step s2 --outcome dropped --start now-30d --end now
```

- Run an inline funnel with `--from-file` holding the request body (see
  `references/request-bodies.md`): `{"appName": ..., "definition": {...}}`.
- Break a funnel down by a dimension with `"breakdown": {"field": "BREAKDOWN_FIELD_PLATFORM", "limit": 5}` in the file.
- `matches` takes the same input as `query`. Only users mode works for now: sessions
  and accounts return 501. User IDs are hashed unless you pass `--user-id-mode raw`.
- To make `matches` agree with a `query`, send the same `--start`/`--end`, filters
  and breakdown to both.

### 4. Pathways and heatmaps

```bash
cx product-analytics pathways query --app web-shop --view /cart --direction after --start now-7d
cx product-analytics heatmaps query --app web-shop --view /checkout --top-n 10 --start now-7d
cx product-analytics events get <eventId> --app web-shop --view /checkout --start now-7d
```

A heatmap element's `eventId` is the same ID `events list` returns.

### 5. Save or change a funnel, pathway or heatmap

```bash
cx product-analytics funnels create --from-file funnel.json --yes
cx product-analytics funnels get 42 -o json > funnel.json   # edit it
cx product-analytics funnels update 42 --from-file funnel.json --yes
cx product-analytics funnels update 42 --from-file funnel.json --update-mask name --yes
cx product-analytics funnels delete 42 --yes
```

`update` changes only the fields in the update mask. Without `--update-mask` the mask
is every top-level field of the file except read-only ones (`id`, `appName`,
`createdAt`, `updatedAt`, `owner`, `version`), so a `get` output can be edited and sent
back. A field in the mask but missing from the file is cleared.

## Filters

Filters go in the JSON file as `"filters": [...]`, combined with AND. Each filter is
exactly one of `stringFilter`, `numericFilter` or `boolFilter`; see
`references/request-bodies.md` for the fields and operators. When running a saved
object, sent filters are added to its saved filters.

## Key principles

- **Start from `applications list` and `views list`** so app and view names are exact.
- **Use `-o json`** when you need to pass IDs, steps or paths to another command.
- **Prefer saved objects** when the user names an existing funnel, pathway or heatmap.
- **Never write without asking**: `create`, `update`, `delete` and `settings update`
  change shared team state.
- **Empty results are not errors** — an app or view with no sessions in the range
  returns empty lists; widen `--start` before concluding there is no traffic.
- **No Product Analytics data** — if `applications list` is empty, the app has no
  RUM sessions in the last 30 days. Do not guess other app names: answer from
  `cx-telemetry-querying` (`rum.events`) and say that Product Analytics needs the RUM
  SDK sending data (`cx-rum-setup`).

## Related Skills

- **`cx-telemetry-querying`** - raw RUM events, errors and web vitals with DataPrime
- **`cx-rum-setup`** - add the RUM SDK to an app so Product Analytics has data

## Additional resources

### Reference files

- **`references/request-bodies.md`** - JSON shapes for filters, time ranges, funnel definitions, breakdowns, and create bodies
