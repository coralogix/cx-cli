---
name: cx-product-analytics
description: >
  Analyze how users move through a web or mobile app with Coralogix Product
  Analytics and the `cx product-analytics` CLI — RUM applications, most visited
  pages, clicks, rage and dead clicks, custom actions, funnels, conversion and
  drop-off, which users converted or dropped, page paths, heatmaps and scroll
  depth. Use when the user asks "where do users drop off", "funnel conversion",
  "conversion rate of checkout", "which users dropped out of the funnel", "what
  pages do users visit after /cart", "user journey", "user paths", "most clicked
  buttons", "rage clicks", "dead clicks", "heatmap", "scroll depth", "top pages",
  "custom events", "create a funnel", "save a pathway", "is product analytics
  enabled", or wants product analytics on RUM data.
metadata:
  version: "0.2.1"
---

# Product Analytics Skill

Product Analytics turns RUM sessions into product questions: which pages users
visit, what they click, where they convert or drop off, and which paths they
take. Use `cx product-analytics` for these questions. For raw RUM events, errors
or web vitals, use `cx-telemetry-querying` (`rum.events`) instead.

## CLI Commands

| Command | Purpose |
|---|---|
| `cx product-analytics applications list` | RUM applications with their platform and session and user counts |
| `cx product-analytics views list --app <app>` | Pages of an app, most visited first |
| `cx product-analytics actions list --app <app> [--type click\|custom]` | Clicks or custom actions of an app, by session count |
| `cx product-analytics filter-values --app <app> --field <FIELD>` | Values of one session field (browser, country, domain, ...) to build filters |
| `cx product-analytics funnels list\|get\|create\|update\|delete` | Saved funnels |
| `cx product-analytics funnels query [id]` | Count and conversion of each step, saved or inline |
| `cx product-analytics funnels matching-users [id] --outcome converted\|dropped` | Users who converted or dropped out (personal data) |
| `cx product-analytics pathways list\|get\|create\|update\|delete` | Saved pathways |
| `cx product-analytics pathways query [id] \| --app <app> --view <view>` | Most common page paths from or to a page |
| `cx product-analytics heatmaps list\|get\|create\|update\|delete` | Saved heatmaps |
| `cx product-analytics heatmaps query [id] \| --app <app> --view <view>` | Most clicked elements on a page, with rage and dead clicks (web only) |
| `cx product-analytics heatmaps scroll-map [id] \| --app <app> --view <view>` | How many users scrolled to each depth of a page (web only) |
| `cx product-analytics settings get` / `update --enabled true\|false` | Whether Product Analytics is on for the team |

- `create`, `update`, `delete` and `settings update` are writes: they need `--yes`
  in agent mode and are blocked by `--read-only`. Ask the user before running them.
  `settings update` also needs the `product-analytics:Enable` permission.
- Every command supports `-o json`, `-o toon` and `-p <profile>` (repeatable).

## Time ranges

Commands that query data take `--start` / `--end` (default `now-1h` / `now`):
`now`, `now-7d`-style relative expressions, or ISO-8601 timestamps. The CLI sends
them as Unix **seconds** (`fromTimestamp` / `toTimestamp`).

When you run a saved funnel, pathway or heatmap by ID, the CLI uses the saved time
range: a fixed range as is, `relativeMinutes` back from now, and
`CALENDAR_WINDOW_TODAY` / `_YESTERDAY` as days in the local timezone, like the UI.
`--start` and `--end` override their side; a lone `--start` or `--end` keeps the other
side of the saved range. The window actually used goes to stderr, e.g.
`Using time range 2026-11-24T00:00:00+02:00 – 2026-11-24T13:05:00+02:00 (saved: today)`;
quote it when you report results.

Saved IDs belong to one team: `get`, `update`, `delete` and running a saved ID accept a
single `-p` profile only.

## Paging

`views list`, `actions list` and `filter-values` return at most one server-configured
page of rows, with no total. If the row count looks like a full page (a round number
such as 100 or 1000), fetch the next page with `--offset <rows so far>` until a page
comes back shorter. `funnels matching-users` takes `--limit` and `--offset`.

## Workflow

### 1. Check access and find the app

```bash
cx product-analytics settings get
cx product-analytics applications list --start now-7d
```

If `settings get` says Product Analytics is not enabled, tell the user; only a team
admin should turn it on (`settings update --enabled true --yes`). For what users did
meanwhile, fall back to `cx-telemetry-querying` (`rum.events`).

### 2. Explore pages, actions and filter values

```bash
cx product-analytics views list --app web-shop --start now-7d
cx product-analytics actions list --app web-shop --start now-7d
cx product-analytics actions list --app web-shop --type custom --start now-7d
cx product-analytics filter-values --app web-shop --field browser-name --start now-7d
```

Pass views exactly as `views list` prints them. `--field` takes a session filter field
(`url-domain`, `platform`, `country-name`, ...; upper case with underscores works too,
full list in `references/request-bodies.md`); use the values it returns in `filters`.

### 3. Funnels

```bash
cx product-analytics funnels list --app web-shop
cx product-analytics funnels query 42 --start now-30d
cx product-analytics funnels query 42 --group-by users
cx product-analytics funnels query --from-file funnel-query.json --start now-7d
cx product-analytics funnels matching-users 42 --outcome dropped --start-step 2 --limit 20
```

- `--group-by` counts sessions (default), users or accounts at each step. `funnels list`
  shows a saved funnel's grouping with the same names (`by users`).
- An inline funnel is a `--from-file` request: `{"query": {"appNames": [...], "steps": [...],
  "filters": [...]}, "breakdown": {"field": "PLATFORM", "limit": 5}}` (see
  `references/request-bodies.md`). The same file works for `matching-users`.
- `matching-users` evaluates from `--start-step` (default 1) to the last step:
  `converted` reached the last step, `dropped` reached the start step but not the last.
  Send the same `--start`/`--end` as the `query` so the two agree.
- **`matching-users` returns real user names, emails, user and account IDs.** Treat
  them as personal data: show only what the user asked for, prefer counts or user IDs
  over names and emails, and never copy them into files, tickets or other tools unless
  the user explicitly asks.

### 4. Pathways and heatmaps

```bash
cx product-analytics pathways query --app web-shop --view /cart --direction forward --start now-7d
cx product-analytics pathways query --app web-shop --view /checkout --direction backward --pages-count 3
cx product-analytics heatmaps query --app web-shop --view /checkout --top-n 10 --start now-7d
cx product-analytics heatmaps scroll-map --app web-shop --view /pricing --start now-7d
cx product-analytics heatmaps query 43
```

### 5. Save or change a funnel, pathway or heatmap

```bash
cx product-analytics funnels create --from-file funnel.json --yes
cx product-analytics funnels get 42 -o json > funnel.json   # edit it
cx product-analytics funnels update 42 --from-file funnel.json --yes
cx product-analytics funnels delete 42 --yes
```

`update` replaces the whole definition: a field missing from the file is cleared
(for example an omitted `breakdown`). `id`, `appName` and `createdAt` are dropped from
the file, so an edited `get` output can be sent back. The app of a saved object cannot
change.

## Filters

Filters go in the JSON file as `"filters": [...]`, combined with AND. Each filter is a
`field` plus one of `stringCondition`, `numericCondition` or `boolCondition`; see
`references/request-bodies.md`. Use `filter-values` to find the values a field takes.

## Key principles

- **Start from `applications list` and `views list`** so app and view names are exact.
- **Use `-o json`** when you need to pass IDs, steps or paths to another command.
- **Prefer saved objects** when the user names an existing funnel, pathway or heatmap.
- **Never write without asking**: `create`, `update`, `delete` and `settings update`
  change shared team state.
- **Protect personal data** from `funnels matching-users` (names, emails, IDs).
- **Empty results are not errors** — an app or view with no sessions in the range
  returns empty lists; widen `--start` before concluding there is no traffic.
- **No Product Analytics data** — if `applications list --start now-30d` is empty, the
  team has no RUM sessions. Do not guess other app names: answer from
  `cx-telemetry-querying` (`rum.events`) and say that Product Analytics needs the RUM
  SDK sending data (`cx-rum-setup`).

## Related Skills

- **`cx-telemetry-querying`** - raw RUM events, errors and web vitals with DataPrime
- **`cx-rum-setup`** - add the RUM SDK to an app so Product Analytics has data

## Additional resources

### Reference files

- **`references/request-bodies.md`** - JSON shapes for filters, funnel steps, saved time ranges, breakdowns, and create bodies
