# CLI Command Grouping Options

## Current State

37 flat top-level commands. Users and agents can't scan `cx --help` to find what they need.

**All 37 commands being reorganized:**

| Command | What it does |
|---|---|
| `logs` | Query logs (DataPrime) |
| `spans` | Query spans (DataPrime) |
| `metrics` | Query metrics (PromQL) |
| `dataprime` | DataPrime reference & raw queries |
| `search-fields` | Semantic field search |
| `dashboards` | Dashboard CRUD + folders |
| `views` | Saved views + view folders |
| `alerts` | Alert definitions CRUD + enable/disable |
| `alert-schedulers` | Alert suppression rules |
| `incidents` | Incident triage (list/ack/resolve) |
| `slos` | SLO definitions CRUD |
| `connectors` | Notification connectors |
| `routers` | Notification routers |
| `presets` | Notification presets |
| `webhooks` | Outgoing webhooks |
| `actions` | Automation hooks |
| `notification-test` | Test notification configs |
| `rule-groups` | Log parsing rule groups |
| `enrichments` | Log enrichment rules |
| `custom-enrichments` | Custom enrichment tables |
| `e2m` | Events-to-Metrics definitions |
| `recording-rules` | Prometheus recording rules |
| `data-usage` | Data consumption metrics |
| `tco-policies` | TCO policy management |
| `retentions` | Data retention settings |
| `quota-rules` | Quota rule management |
| `data-archive` | Archive storage config |
| `integrations` | Third-party integrations |
| `extensions` | Extensions management |
| `contextual-data` | Contextual data integrations |
| `api-keys` | API key management |
| `roles` | Custom & system roles |
| `scopes` | Team scopes |
| `users` | User management |
| `team-groups` | Team group management |
| `saml` | SAML configuration |
| `ip-access` | IP access restrictions |
| `profiles` | Local profile management |
| `cleanup` | Remove stale temp files |

---

## Option A: Six Domain Categories

Group by Coralogix product domain. Each category maps to a distinct area of the platform.

### Structure

```
cx --help

Query & Explore:
  logs             Query logs using DataPrime syntax
  spans            Query spans using DataPrime syntax
  metrics          Query metrics using PromQL
  dataprime        DataPrime language reference and raw queries
  search-fields    Search log/span fields semantically
  dashboards       Manage dashboards and dashboard folders
  views            Manage saved views and view folders

Management:
  alerting         Alerts, incidents, schedulers, and SLOs
  notifications    Connectors, routers, presets, webhooks, and actions
  pipeline         Parsing rules, enrichments, E2M, and recording rules
  cost             Data usage, TCO policies, retentions, quotas, and archive
  iam              API keys, roles, scopes, users, groups, SAML, and IP access
  integrations     Integrations, extensions, and contextual data

Local:
  profiles         Manage profiles (list, add, delete, set-default)
  cleanup          Remove stale temp files
```

### What goes where

| Category | Commands absorbed | Renames |
|---|---|---|
| _root_ (unchanged) | logs, spans, metrics, dataprime, search-fields, dashboards, views, profiles, cleanup | — |
| `alerting` | alerts, alert-schedulers, incidents, slos | alert-schedulers → schedulers |
| `notifications` | connectors, routers, presets, webhooks, actions, notification-test | notification-test → test |
| `pipeline` | rule-groups, enrichments, custom-enrichments, e2m, recording-rules | rule-groups → rules |
| `cost` | data-usage, tco-policies, retentions, quota-rules, data-archive | data-usage → usage, data-archive → archive |
| `iam` | api-keys, roles, scopes, users, team-groups, saml, ip-access | team-groups → groups |
| `integrations` | integrations (CRUD), extensions, contextual-data | — |

**Top-level count:** 16 (9 query/local + 6 categories + help)

### Pros

- **Precise categories** — each group has a tight, obvious theme; no "where does X go?" ambiguity
- **Matches Coralogix platform mental model** — users familiar with the Coralogix UI can predict where to look
- **Good for agents** — an AI agent investigating an alert can `cx alerting --help` and see everything relevant without noise from IAM or pipeline commands
- **Categories are self-describing** — `iam`, `cost`, `pipeline` are industry-standard terms that need no explanation

### Cons

- **Still 16 top-level items** — better than 37, but could be tighter
- **Some categories are small** — `integrations` has only 3 sub-groups, `cost` has 5; arguably not worth their own category
- **Depth increases** — `cx alerting alerts list` is 3 levels deep for a common operation
- **"notifications" vs "alerting" split isn't obvious** — users may look for webhooks under alerting, or alerts under notifications

---

## Option B: Three Broad Groups

Minimize categories by grouping along the user's workflow: **observe** (query data), **manage** (configure the platform), **admin** (governance & access).

### Structure

```
cx --help

Observe:
  logs             Query logs using DataPrime syntax
  spans            Query spans using DataPrime syntax
  metrics          Query metrics using PromQL
  dataprime        DataPrime language reference and raw queries
  search-fields    Search log/span fields semantically

Manage:
  dashboards       Manage dashboards and dashboard folders
  views            Manage saved views and view folders
  alerts           Manage alerts, incidents, schedulers, and SLOs
  notifications    Manage connectors, routers, presets, webhooks, and actions
  pipeline         Manage parsing rules, enrichments, E2M, and recording rules
  integrations     Manage integrations, extensions, and contextual data

Admin:
  governance       Data usage, TCO policies, retentions, quotas, and archive
  access           API keys, roles, scopes, users, groups, SAML, and IP access

Local:
  profiles         Manage profiles (list, add, delete, set-default)
  cleanup          Remove stale temp files
```

### What goes where

| Category | Commands absorbed | Renames |
|---|---|---|
| _root_ (unchanged) | logs, spans, metrics, dataprime, search-fields, profiles, cleanup | — |
| `dashboards` (unchanged) | dashboards | — |
| `views` (unchanged) | views | — |
| `alerts` | alerts, alert-schedulers, incidents, slos | alert-schedulers → schedulers |
| `notifications` | connectors, routers, presets, webhooks, actions, notification-test | notification-test → test |
| `pipeline` | rule-groups, enrichments, custom-enrichments, e2m, recording-rules | rule-groups → rules |
| `integrations` | integrations (CRUD), extensions, contextual-data | — |
| `governance` | data-usage, tco-policies, retentions, quota-rules, data-archive | data-usage → usage, data-archive → archive |
| `access` | api-keys, roles, scopes, users, team-groups, saml, ip-access | team-groups → groups |

**Top-level count:** 14 (5 query + 4 manage groups + 2 admin groups + 2 local + help)

### Pros

- **Fewest categories** — only 2 new grouping names to learn (`governance`, `access`); the rest stay top-level or use obvious names
- **Mirrors user workflow** — observe → manage → admin matches how people actually use a CLI day-to-day
- **Alerts stay top-level** — the most common management command doesn't get buried behind a category
- **Easy for agents** — broad categories with clear names; an agent can quickly narrow "I need to check cost stuff" → `cx governance`

### Cons

- **Still 14 top-level items** — not much reduction from Option A's 16
- **"governance" is vague** — users may not intuit that retentions and TCO live there
- **"access" is ambiguous** — could mean "data access" vs "user access"; `iam` is more precise
- **The "manage" group is really just 4 mini-categories at root** — it's not actually a group, just a help-text heading. Semantically similar to Option A but with `cost`/`iam` renamed

---

## Option C: Observe + Configure (Two-Tier Split)

Only two real categories: keep everything users **query** at the top level, and put everything users **configure** under a single `config` supercommand. This mirrors how cloud CLIs like `gcloud` work — the product commands are top-level, the management API is behind `gcloud config` / `gcloud iam` / etc.

### Structure

```
cx --help

  logs             Query logs using DataPrime syntax
  spans            Query spans using DataPrime syntax
  metrics          Query metrics using PromQL
  dataprime        DataPrime language reference and raw queries
  search-fields    Search log/span fields semantically
  dashboards       Manage dashboards and dashboard folders
  views            Manage saved views and view folders
  alerts           Manage alerts and alert definitions
  incidents        Manage and triage incidents
  slos             Manage SLO definitions
  config           Configure platform settings (run cx config --help)
  profiles         Manage profiles (list, add, delete, set-default)
  cleanup          Remove stale temp files

cx config --help

  Notifications:
    connectors         Manage notification connectors
    routers            Manage notification routers
    presets            Manage notification presets
    webhooks           Manage outgoing webhooks
    actions            Manage automation hooks
    notification-test  Test notification configurations

  Pipeline:
    rules              Manage log parsing rule groups
    enrichments        Manage log enrichment rules
    custom-enrichments Manage custom enrichment tables
    e2m                Manage Events2Metrics definitions
    recording-rules    Manage Prometheus recording rules

  Cost & Storage:
    usage              View data usage and consumption metrics
    tco-policies       Manage TCO policies
    retentions         Manage data retention settings
    quota-rules        Manage quota rules
    archive            Manage data archive storage

  Access & Identity:
    api-keys           Manage API keys
    roles              Manage custom and system roles
    scopes             Manage team scopes
    users              Search and manage users
    groups             Manage team groups
    saml               Manage SAML configuration
    ip-access          Manage IP access restrictions

  Integrations:
    integrations       Manage third-party integrations
    extensions         Manage extensions
    contextual-data    Manage contextual data integrations

  Scheduling:
    schedulers         Manage alert scheduler (suppression) rules
```

### What goes where

| Category | Commands absorbed | Renames |
|---|---|---|
| _root_ (unchanged) | logs, spans, metrics, dataprime, search-fields, dashboards, views, profiles, cleanup | — |
| _root_ (stays top-level) | alerts, incidents, slos | — |
| `config` → notifications | connectors, routers, presets, webhooks, actions, notification-test | — |
| `config` → pipeline | rule-groups, enrichments, custom-enrichments, e2m, recording-rules | rule-groups → rules |
| `config` → cost | data-usage, tco-policies, retentions, quota-rules, data-archive | data-usage → usage, data-archive → archive |
| `config` → access | api-keys, roles, scopes, users, team-groups, saml, ip-access | team-groups → groups |
| `config` → integrations | integrations, extensions, contextual-data | — |
| `config` → scheduling | alert-schedulers | alert-schedulers → schedulers |

**Top-level count:** 14 (12 commands + config + help)

### Pros

- **Top level is clean and action-oriented** — everything at root is something you DO regularly (query, view, triage)
- **Single bucket for "admin stuff"** — users don't need to guess which of 6 categories holds a setting; it's all under `config`
- **Great for agents** — an agent doing incident response sees alerts/incidents/slos immediately; it only dives into `config` when it needs to change platform settings
- **Low learning curve** — new users only need to learn one new concept: "day-to-day commands are at root, platform configuration is under `cx config`"
- **Alerts, incidents, SLOs stay top-level** — these are operational commands used in incident response, not "configuration"

### Cons

- **`config` becomes a junk drawer** — 28 commands under one umbrella; `cx config --help` is now the new "too many commands" problem (mitigated by sub-headings)
- **3 levels deep for common admin tasks** — `cx config api-keys list` is more typing than `cx iam api-keys list`
- **"config" name collision risk** — users might confuse `cx config` (platform configuration) with `cx profiles` (local CLI configuration)
- **The sub-headings inside `config` are basically Option A's categories** — you're just adding one more level of indirection

---

## Option D: Expert Flat Domains (pup-style)

Assume the user is an observability expert. No category wrappers — every domain is a top-level command with a precise, industry-familiar name. Reduce the count not by grouping but by **merging closely related commands** into single domains with subcommands. Inspired by Datadog's [pup CLI](https://github.com/datadog-labs/pup), which has 55+ flat top-level domains and zero category nesting.

The philosophy: observability engineers already know what "e2m" means, what "tco" means, what "slos" are. They don't need a `cost` category to tell them that `tco` is about cost. They just type `cx tco`.

### Structure

```
cx --help

Query:
  logs               Query logs using DataPrime syntax
  spans              Query spans using DataPrime syntax
  metrics            Query metrics using PromQL
  dataprime          DataPrime language reference and raw queries
  search-fields      Search log/span fields semantically

Observe:
  dashboards         Manage dashboards and dashboard folders
  views              Manage saved views and view folders
  slos               Manage SLO definitions

Detect & Respond:
  alerts             Manage alert definitions and schedulers
  incidents          Manage and triage incidents

Notifications:
  notifications      Manage connectors, routers, presets, and notification testing
  webhooks           Manage outgoing webhooks and automation actions

Data Pipeline:
  rules              Manage log parsing rule groups
  enrichments        Manage enrichment rules and custom enrichment tables
  e2m                Manage Events2Metrics definitions
  recording-rules    Manage Prometheus recording rule groups

Cost & Storage:
  usage              View data usage and consumption metrics
  tco                Manage TCO policies and settings
  retentions         Manage data retention settings
  quotas             Manage quota rules
  archive            Manage data archive storage configuration

Integrations:
  integrations       Manage integrations, extensions, and contextual data

Access:
  iam                Manage API keys, roles, scopes, users, groups, SAML, and IP access

Local:
  profiles           Manage profiles (list, add, delete, set-default)
  cleanup            Remove stale temp files
```

Note: the headings above (Query, Observe, etc.) are **help-text display groups only** — not actual command prefixes. Every command is typed directly as `cx <command>`. Clap supports this via `#[command(next_help_heading = "...")]`.

### What goes where

Every command is top-level. The reduction from 37 → 24 comes from merging related commands:

| New command | Absorbs | How |
|---|---|---|
| `alerts` | alerts + alert-schedulers | `cx alerts list`, `cx alerts schedulers list\|get\|create\|delete` |
| `notifications` | connectors + routers + presets + notification-test | `cx notifications connectors list`, `cx notifications routers list`, `cx notifications test ...` |
| `webhooks` | webhooks + actions | `cx webhooks list`, `cx webhooks actions list\|get\|create\|delete` |
| `rules` | rule-groups | rename only: `cx rules list` instead of `cx rule-groups list` |
| `enrichments` | enrichments + custom-enrichments | `cx enrichments list`, `cx enrichments custom list\|get\|create\|delete` |
| `tco` | tco-policies | rename only: `cx tco list` instead of `cx tco-policies list` |
| `quotas` | quota-rules | rename only: `cx quotas get` instead of `cx quota-rules get` |
| `usage` | data-usage | rename only: `cx usage summary` instead of `cx data-usage summary` |
| `archive` | data-archive | rename only: `cx archive metrics get` instead of `cx data-archive metrics get` |
| `integrations` | integrations + extensions + contextual-data | `cx integrations list`, `cx integrations extensions list`, `cx integrations contextual-data list` |
| `iam` | api-keys + roles + scopes + users + team-groups + saml + ip-access | `cx iam api-keys list`, `cx iam roles list`, `cx iam users search`, `cx iam groups list`, `cx iam saml get`, `cx iam ip-access get` |
| _all others_ | unchanged | logs, spans, metrics, dataprime, search-fields, dashboards, views, incidents, slos, e2m, recording-rules, retentions, profiles, cleanup |

**Top-level count:** 25 (24 commands + help)

### Example usage

```bash
# Daily querying — no change
cx logs 'filter $m.severity == ERROR'
cx spans 'filter $l.serviceName == "checkout"' --start now-2h
cx metrics query 'rate(http_requests_total[5m])'

# Alert management — alerts + schedulers merged
cx alerts list
cx alerts get <id>
cx alerts schedulers list

# Incident response — top-level, one keystroke away
cx incidents list --severity CRITICAL
cx incidents acknowledge <id>
cx slos list

# Notification config — connectors/routers/presets merged
cx notifications connectors list
cx notifications routers list
cx notifications test ...

# Webhooks + actions merged
cx webhooks list
cx webhooks actions list

# Pipeline — shorter names
cx rules list
cx enrichments list
cx enrichments custom list
cx e2m list
cx recording-rules list

# Cost — each domain is top-level, no wrapper
cx usage summary
cx tco list
cx retentions list
cx quotas get
cx archive metrics get

# IAM — single domain for all access management
cx iam api-keys list
cx iam roles list
cx iam users search
cx iam groups list
cx iam saml get
cx iam ip-access get

# Integrations — merged
cx integrations list
cx integrations extensions list
cx integrations contextual-data list
```

### Pros

- **Lowest max depth** — most commands are only 2 levels (`cx rules list`, `cx tco list`, `cx usage summary`). Even merged commands are max 3 (`cx alerts schedulers list`). Compare to Option A's `cx alerting alerts list` which is 3 levels for the most common alert operation.
- **Zero new abstractions** — no "alerting" wrapper, no "pipeline" wrapper, no "cost" wrapper. Users type the domain name they already know. An observability engineer thinks "TCO" not "cost → tco-policies".
- **Fast for experts** — fewest keystrokes for the most common operations. `cx tco list` vs `cx cost tco-policies list` (Option A) vs `cx config tco-policies list` (Option C).
- **pup-proven pattern** — Datadog ships 55+ flat domains to observability professionals and it works. cx's 24 is even more manageable.
- **Great for agents** — flat domain names are easy to match from natural language. "Show me TCO policies" → `cx tco list`. No need to figure out which category "TCO" lives under.
- **Help-text headings provide discoverability without hierarchy** — `cx --help` groups commands visually, but the user never types a group name. Best of both worlds.
- **Shorter, more memorable command names** — `rules` not `rule-groups`, `tco` not `tco-policies`, `quotas` not `quota-rules`, `usage` not `data-usage`

### Cons

- **25 top-level commands** — more than Options A/B/C. Still a long `--help` output (though organized with headings)
- **Requires domain expertise** — a new user won't know that `e2m` means Events-to-Metrics or that `tco` is about cost optimization. No category wrapper to provide context.
- **Merging rules are case-by-case** — "why are connectors under `notifications` but webhooks are separate?" Each merge decision needs justification and feels somewhat arbitrary.
- **`iam` is doing a lot of work** — 7 sub-domains under one command; it's a mini-category disguised as a domain. If `iam` gets special treatment, why not `pipeline` or `cost`?
- **Less scalable for new commands** — without categories, adding a 26th or 30th command makes the flat list grow. With Option A, new commands slot into existing categories silently.

---

## Option E: Coralogix-Native (mirror the platform)

Based on how [Coralogix's own documentation](https://coralogix.com/docs/user-guides/) organizes features. The grouping follows the platform's information architecture — so a user who knows the Coralogix UI already knows where to look in the CLI.

The key insight: Coralogix's docs make **different grouping decisions** than all our previous options:

| Concept | Options A-D put it in | Coralogix docs put it under |
|---|---|---|
| E2M, recording-rules | "pipeline" / standalone | **Metrics** |
| Outbound webhooks | "notifications" / standalone | **Alerting** |
| TCO, usage, quotas, retentions, archive | "cost" / standalone | **Account Management** |
| Parsing rules, enrichments | "pipeline" | **Data Transformation** |
| Actions (automation hooks) | "notifications" | **Alerting** (close to webhooks) |
| SLOs | grouped with alerts | **Standalone section** |
| Connectors, routers, presets | "notifications" | **Notification Center** (separate from Alerting) |

These aren't arbitrary — they reflect how Coralogix engineers and users actually think about the product.

### Structure

Uses Option D's approach (flat domains + help-text headings, no category prefixes) but with Coralogix-aligned merges and headings:

```
cx --help

Query:
  logs               Query logs using DataPrime syntax
  spans              Query spans using DataPrime syntax
  metrics            Query metrics, manage E2M and recording rules
  dataprime          DataPrime language reference and raw queries
  search-fields      Search log/span fields semantically

Explore:
  dashboards         Manage dashboards and dashboard folders
  views              Manage saved views and view folders

Detect & Respond:
  alerts             Manage alerts, schedulers, webhooks, and actions
  incidents          Manage and triage incidents
  slos               Manage SLO definitions

Notification Center:
  notifications      Manage connectors, routers, and presets

Data Transformation:
  rules              Manage log parsing rule groups
  enrichments        Manage enrichment rules and custom enrichment tables

Account:
  account            API keys, roles, scopes, users, groups, SAML, IP access, TCO, usage, quotas, retentions, and archive

Integrations:
  integrations       Manage integrations, extensions, and contextual data

Local:
  profiles           Manage profiles (list, add, delete, set-default)
  cleanup            Remove stale temp files
```

Like Option D, headings are **display-only** — every command is typed directly as `cx <command>`.

### What goes where

| New command | Absorbs | Rationale (from Coralogix docs) |
|---|---|---|
| `metrics` | metrics + e2m + recording-rules | Coralogix docs: E2M and recording rules are under "Metrics", not "Pipeline" — they produce/transform metrics |
| `alerts` | alerts + alert-schedulers + webhooks + actions | Coralogix docs: outbound webhooks and actions are alert delivery mechanisms, grouped under "Alerting" |
| `notifications` | connectors + routers + presets + notification-test | Coralogix docs: "Notification Center" is the routing/connector layer, separate from alerting |
| `rules` | rule-groups | Coralogix docs: "Parsing" is under "Data Transformation" |
| `enrichments` | enrichments + custom-enrichments | Coralogix docs: "Data Enrichment" groups both standard and custom enrichments |
| `account` | api-keys, roles, scopes, users, team-groups, saml, ip-access, tco-policies, data-usage, quota-rules, retentions, data-archive | Coralogix docs: ALL of these are under "Account Management" — TCO Optimizer, billing, access control, user management |
| `integrations` | integrations + extensions + contextual-data | Coralogix docs: top-level "Integrations" section |
| _unchanged_ | logs, spans, dataprime, search-fields, dashboards, views, incidents, slos, profiles, cleanup | — |

**Top-level count:** 17 (16 commands + help)

### Example usage

```bash
# Querying — unchanged
cx logs 'filter $m.severity == ERROR'
cx metrics query 'rate(http_requests_total[5m])'

# Metrics now includes E2M and recording rules
cx metrics query 'up'
cx metrics e2m list
cx metrics e2m create --from-file e2m.json
cx metrics recording-rules list
cx metrics recording-rules create --from-file rules.json

# Alerts include schedulers, webhooks, and actions
cx alerts list
cx alerts schedulers list
cx alerts webhooks list
cx alerts webhooks types
cx alerts actions list
cx alerts actions create --from-file action.json

# Notification Center — connectors/routers/presets
cx notifications connectors list
cx notifications routers list
cx notifications presets list
cx notifications test ...

# Data transformation
cx rules list
cx enrichments list
cx enrichments custom list

# Account — single domain for all admin + cost
cx account api-keys list
cx account roles list
cx account users search
cx account groups list
cx account saml get
cx account ip-access get
cx account tco list
cx account usage summary
cx account quotas get
cx account retentions list
cx account archive metrics get

# Incidents and SLOs — top-level
cx incidents list --severity CRITICAL
cx slos list
```

### Pros

- **Mirrors Coralogix's own mental model** — users who know the UI already know where to find things. No new vocabulary to learn. The CLI is a 1:1 projection of the platform's information architecture.
- **Smarter merges than Option D** — E2M and recording-rules under `metrics` makes domain sense (they're metric operations). Webhooks under `alerts` makes domain sense (they're alert delivery). Option D's groupings were somewhat arbitrary by comparison.
- **Fewest top-level commands** (17) of any flat option — achieves the same flat-domain benefits as Option D (no category prefixes to type) but with fewer entries in `--help`.
- **`metrics` becomes a power command** — an engineer working with metrics can discover queries, E2M, and recording rules all in one place. This matches how Coralogix positions metrics as a unified feature area.
- **Agents benefit from platform alignment** — AI agents that also have access to Coralogix docs or UI context will find the CLI's structure predictable. "Coralogix docs say webhooks are under Alerting" → `cx alerts webhooks list` just works.

### Cons

- **`account` is a mega-command** — 12 sub-domains under one command. This is worse than Option D's `iam` (7 sub-domains). `cx account --help` will be long. Coralogix gets away with this in docs because it has rich navigation; a CLI `--help` page is flatter.
- **Webhooks under `alerts` is counterintuitive for non-Coralogix users** — someone coming from PagerDuty or Grafana would look for webhooks under notifications. This only makes sense if you know Coralogix's specific product architecture.
- **`metrics` doing double duty** — `cx metrics query` (query data) vs `cx metrics e2m list` (manage config) are very different operations sharing a namespace. Could confuse users who think of `metrics` as "query metrics."
- **Tight coupling to Coralogix's current docs structure** — if Coralogix reorganizes their docs (which happens), the CLI becomes misaligned. Options A and D are based on industry concepts that don't change.
- **`notifications` becomes thin** — only connectors, routers, presets, and test. With webhooks moved to alerts, this is 4 sub-commands. Arguably not worth its own top-level entry.

---

## Comparison Matrix

| Criterion | A: Six Domains | B: Three Groups | C: Observe + Config | D: Expert Flat | E: Coralogix-Native |
|---|---|---|---|---|---|
| Top-level count | 16 | 14 | 14 | 25 | 17 |
| Max depth | 3 (`cx alerting alerts list`) | 3 (`cx alerts schedulers list`) | 3 (`cx config api-keys list`) | 3 (`cx iam api-keys list`) but most are 2 | 3 (`cx account api-keys list`) but most are 2 |
| Discoverability | High — precise names | Medium — broader names | High — root = action, config = admin | High for experts — domain names are the commands | High — matches Coralogix UI/docs |
| Learnability | Medium — 6 new terms | High — 2 new terms | High — 1 new concept | Low — assumes domain vocabulary | Low for CX users (they know the UI), low for others |
| Agent-friendliness | High — narrow search space per category | High — fewer choices | High — clear "query vs. configure" split | Very high — flat names map directly from natural language | High — agents with CX context find it predictable |
| Risk of "wrong bucket" | Low — tight categories | Medium — `governance` is vague | Low — binary split is intuitive | None — no buckets to be wrong about | Low if you know CX, medium otherwise |
| Typing overhead | Medium | Low | Medium (config prefix) | Lowest — fewest keystrokes for most commands | Low — flat with smart merges |
| Scalability (new commands) | Good — clear where new commands go | OK — groups may get lopsided | Good — new config always under `config` | OK — flat list grows, but help headings mitigate | Good — follows CX product evolution |

---

## Decision: Option D (Expert Flat Domains) + `cx schema` command

**Status:** Decided 2026-04-28

### Chosen approach

**Option D** — flat domains with help-text headings, no category prefixes. 24 top-level commands with smart merges and shorter names. Plus a new `cx schema` command that outputs the full command tree as structured JSON for agent consumption.

### Why Option D

We evaluated 5 options (A through E) across multiple dimensions. Option D won on the criteria that matter most for cx's audience and use cases:

1. **Best for agents with zero Coralogix knowledge.** Flat domain names map 1:1 from natural language — an agent hearing "check TCO policies" invokes `cx tco list` directly. No intermediate "which category is TCO in?" lookup. Every other option requires the agent to either navigate a hierarchy (A, B, C) or know Coralogix-specific product groupings (E). Option D's names are industry-standard and self-descriptive.

2. **Best for cross-platform users.** Observability engineers coming from Datadog, Grafana, or New Relic find universal domain names (`tco`, `e2m`, `rules`, `enrichments`, `iam`) where they'd expect them. Option E's Coralogix-specific merges (webhooks under alerts, TCO under account) would confuse anyone who hasn't internalized Coralogix's product architecture.

3. **Fewest keystrokes for common operations.** Most commands are 2 tokens (`cx tco list`, `cx rules list`, `cx alerts list`). Option A's `cx alerting alerts list` adds a token for the most common management command. This compounds across a workday of CLI usage.

4. **Help-text headings give discoverability without depth.** `cx --help` visually groups commands (Query, Detect & Respond, Cost & Storage, etc.) but the user never types a group name. This is the key insight from Datadog's pup CLI: organize the help output without adding levels to the command tree.

### Why not the others

- **Option A (Six Domains):** Adds unnecessary depth. `cx alerting alerts list` is 3 tokens for the most common management command. Categories are precise but patronizing for expert users.
- **Option B (Three Groups):** `governance` and `access` are vague. An agent can't reliably guess which one holds SAML.
- **Option C (Observe + Config):** `config` becomes a 28-command junk drawer. Moves the problem one level down instead of solving it.
- **Option E (Coralogix-Native):** `account` mega-command (12 sub-domains) is too heavy. Merges like "webhooks under alerts" and "E2M under metrics" only make sense if you know Coralogix's product architecture — penalizes both cross-platform users and zero-knowledge agents.

### `cx schema` command

Additionally, we will add a `cx schema` command (inspired by pup's `pup agent schema`) that outputs the entire CLI command tree as structured JSON. This gives AI agents (Cursor, Claude Code) structured tool definitions in a single call — no help-text parsing needed. This is orthogonal to the grouping choice but significantly improves agent UX.

### What happens next

The implementation plan in `plans/cli-command-hierarchy.plan.md` needs to be updated to reflect Option D's structure (it currently reflects Option A). The plan's milestones and task breakdown remain valid — only the specific enum definitions, merge decisions, and naming need to change.
