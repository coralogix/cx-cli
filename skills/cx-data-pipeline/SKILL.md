---
name: cx-data-pipeline
description: >
  Use this skill when the user asks to "set up parsing", "create parsing rule",
  "extract fields from logs", "regex extraction", "log parsing",
  "enrich logs", "add context to logs", "custom enrichment table", "lookup table",
  "geo enrichment", "create metric from logs", "events to metrics",
  "convert logs to metrics", "generate metrics from events",
  "recording rule", "precomputed metrics", "PromQL recording",
  "configure data pipeline", "transform log data", "data processing rules",
  "rule group", "enrichment settings", "E2M definition", "labels cardinality",
  "bulk delete rules", "enrichment limits", "search enrichment table",
  "what should I convert to metrics", "E2M not producing metrics", "E2M no series",
  "reduce log cost with E2M", "logs to metrics aggregation", "spans to metrics",
  or wants to configure how Coralogix processes, enriches, or transforms ingested data.
metadata:
  version: "0.1.0"
---

# Data Pipeline Skill

Use this skill when configuring how Coralogix processes, enriches, and transforms data. It covers parsing rules (extract structured fields from raw logs), enrichments (add context from lookup tables), Events2Metrics (derive metrics from log/span events), and recording rules (precompute PromQL expressions).

---

## CLI Commands

| Command | Subcommands | Purpose |
|---|---|---|
| `cx parsing-rules` | `list`, `get`, `create`, `update`, `delete`, `bulk-delete`, `usage-limits` | Manage log parsing rules |
| `cx enrichments` | `list`, `add`, `remove`, `overwrite`, `limit`, `settings` | Manage enrichment rules |
| `cx enrichments custom` | `list`, `get`, `create`, `update`, `delete`, `search` | Manage custom enrichment tables |
| `cx e2m` | `list`, `get`, `create`, `update`, `delete`, `labels-cardinality`, `limits` | Manage Events2Metrics definitions |
| `cx recording-rules` | `list`, `get`, `create`, `update`, `delete` | Manage Prometheus recording rule groups |

Key flags:
- All create/update operations use `--from-file <path>` (or `-` for stdin)
- All commands support `-o json` for structured output and `-p <profile>` for profile selection
- `cx parsing-rules update` and `cx recording-rules update` require both `--from-file` and the rule group ID
- `cx enrichments custom search` requires `--id <table-id>` and `--query <text>`
- `cx parsing-rules bulk-delete` requires `--ids <id1> <id2> ...`

---

## Working with JSON Payloads

These commands use complex JSON structures. **Always template from an existing resource** to avoid format errors:

```bash
# 1. Get an existing resource as a template
cx parsing-rules get <rule-group-id> -o json > template.json

# 2. Modify the template (change fields, remove the ID for create operations)

# 3. Create or update
cx parsing-rules create --from-file template.json
cx parsing-rules update --from-file template.json <rule-group-id>
```

This pattern applies to all create/update operations across all 4 commands. It prevents payload format errors that are the #1 cause of failed attempts.

---

## Parsing Rules Workflow

### 1. List Existing Rules

```bash
cx parsing-rules list -o json
cx parsing-rules list -o json | jq '[.[] | {id, name, enabled, rule_count: (.rules | length)}]'
```

### 2. Get a Template

```bash
cx parsing-rules get <existing-rule-group-id> -o json > rule-template.json
```

### 3. Create New Rule Group

Edit the template for your new service, then:

```bash
cx parsing-rules create --from-file rule-template.json
```

### 4. Verify Parsing

Query recent logs to confirm fields are extracted (load `cx-telemetry-querying` for log querying):

```bash
cx logs 'source logs | filter $d.subsystem == "my-service" | limit 10' -o json
```

### 5. Check Usage Limits

```bash
cx parsing-rules usage-limits -o json
```

---

## Enrichment Workflow

### 1. List Enrichment Rules

```bash
cx enrichments list -o json
cx enrichments settings -o json
cx enrichments limit -o json
```

### 2. Create Custom Enrichment Table (if needed)

```bash
cx enrichments custom list -o json
cx enrichments custom create --from-file table-definition.json
```

`table-definition.json` must use the v5 JSON shape (inline file content, not multipart `file=@...`):

```json
{
  "name": "IP Lookup",
  "description": "Maps IPs to locations",
  "file": {
    "textual": "ip,city\n1.2.3.4,London",
    "extension": "csv",
    "name": "lookup.csv",
    "size": 24
  }
}
```

For updates, include `customEnrichmentId` (number) plus the same fields.

### 3. Add Enrichment Rules

```bash
cx enrichments add --from-file enrichment-rules.json
```

`enrichment-rules.json` must use `requestEnrichments` (not `enrichments` from list output). Each `enrichmentType` is an object, not a string:

```json
{
  "requestEnrichments": [
    {
      "fieldName": "sourceIPs",
      "enrichmentType": { "geoIp": { "withAsn": true } }
    }
  ]
}
```

Other types: `{"aws": {"resourceType": "ec2"}}`, `{"suspiciousIp": {}}`, `{"customEnrichment": {"id": 1}}`.

### 4. Search Custom Table Data

```bash
cx enrichments custom search --id <table-id> --query "search term"
```

### 5. Verify Enriched Fields

Query logs on hot storage (FrequentSearch tier) to confirm enriched fields appear. Avoid querying archive for verification - ingestion delays can cause false negatives.

```bash
cx logs 'source logs | filter $d.enriched_field != null | limit 5' -o json
```

---

## Events2Metrics Workflow

E2M derives Prometheus metrics from log/span events. See **[`references/e2m-schemas.md`](references/e2m-schemas.md)** for the full JSON wire format, enums, and cardinality rules.

### How E2M is computed (read this first)

E2M aggregates events **as they stream through the real-time ingestion pipeline** into metric series (~1-min resolution). It is **forward-only** — metrics start from the moment the E2M is created; there is **no backfill**.

All ingested data flows through the pipeline; a **TCO policy** routes each stream into a tier, and the tier decides what's possible:

| TCO tier | Storage | E2M / alerts / dashboards |
|---|---|---|
| **High** | Frequent Search (hot, OpenSearch) | ✅ available |
| **Medium** | S3 archive (not hot storage) | ✅ available — still processed by the pipeline |
| **Low** | Compliance only | ❌ no aggregation features |
| **Blocked** | dropped | ❌ |

The axis is **tier / processing level — NOT "Frequent Search vs archive"** (Medium *is* archive and E2M works on it). Do not tell users to "point E2M at archive instead of Frequent Search" — that is incorrect.

### 1. Design the metric

Choose `logs2metrics` vs `spans2metrics`, the source field(s) + aggregations, and labels (with cardinality in mind — see `references/e2m-schemas.md`). To scope the E2M to a **dataset**, set the optional `dataSource` field to `"<dataspace>/<dataset>"`; this requires the account feature `e2m_dataset_source_enabled` (otherwise the API rejects it with "dataSource is not enabled for this company"). Omit it for the standard logs/spans stream.

### 2. Size it: check limits & cardinality

```bash
cx e2m limits -o json              # account E2M count limit + used
cx e2m labels-cardinality -o json  # see caveat below
```

The labels-cardinality endpoint is a **draft forecast** — given proposed labels + query it returns the per-day distinct-permutation count over the last 7 days, so you can size a design before creating it. **But `cx e2m labels-cardinality` currently takes no arguments**, so it sends no draft and returns an empty list (a CLI gap — it can't forecast yet). Until that's wired up, forecast via the UI or estimate permutations manually (product of distinct label values) and set `permutationsLimit`. Never use high-cardinality fields (IDs, raw URLs, IPs) as labels. Note the forecast only sees Frequent-Search (High-tier) data.

### 3. Template from an existing definition

Only `cx e2m get` returns the full payload (`{"e2m": {...}}`); `list` prints a summary. Extract `.e2m` and drop read-only fields:

```bash
cx e2m get <existing-e2m-id> -o json | jq '.e2m | del(.id, .permutations, .createTime, .updateTime, .metricName)' > e2m.json
```

### 4. Create the E2M

```bash
cx e2m create --from-file e2m.json
```

### 5. Verify the metric

Confirm series are being produced (load `cx-telemetry-querying` for metrics querying):

```bash
cx metrics search --name "<targetBaseMetricName>"
cx metrics query "<target_metric_name>" --time now
```

### Troubleshooting: E2M produces no metric series

1. **Check the source data's TCO tier** — if it's routed to **Low/compliance** (or blocked), E2M cannot run. Fix with a TCO change (`cx tco list` / `cx-cost-optimization`), **not** an E2M change.
2. **Verify the query matches streaming data** — run the E2M's `lucene` filter as a live `cx logs`/`cx spans` query and confirm it returns recent results. Note `cx logs` queries Frequent-Search (High-tier) by default; for a **Medium-tier (archive)** source add `--tier archive`, since the data won't appear in a default Frequent-Search query even though E2M still produces series.
3. **Remember it's forward-only** — no series exist for data ingested before the E2M was created.

### Cost optimization: convert High-tier logs to metrics

When the **aggregated/metric view is what the customer most cares about**, convert **High-tier logs → metrics**, then downgrade the raw logs **High → Medium**. Medium still supports E2M/alerts/dashboards and costs less (S3 archive, no hot storage) — you keep cheap, detailed metrics while dropping expensive Frequent-Search retention.

1. Find high-volume High-tier sources: `cx usage summary` / `cx tco list` (see `cx-cost-optimization`).
2. Confirm which fields drive dashboards/alerts (see `cx-telemetry-querying`).
3. Build + **verify the E2M first** (steps above).
4. **Then** change the TCO policy to move the raw logs High → Medium. Keep data on High or Medium (both support E2M); do **not** drop it to Low/compliance if metrics or alerts are still needed.

---

## Recording Rules Workflow

### 1. List Existing Recording Rules

```bash
cx recording-rules list -o json
cx recording-rules list -o json | jq '[.[] | {id, name, rules: [.rules[]?.record]}]'
```

### 2. Get a Template

```bash
cx recording-rules get <existing-id> -o json > recording-rule-template.json
```

### 3. Create Recording Rule Group

```bash
cx recording-rules create --from-file recording-rule-group.json
```

### 4. Verify with PromQL

Confirm the precomputed metric is available (load `cx-telemetry-querying` for metrics querying):

```bash
cx metrics query "new_precomputed_metric" --time now
```

---

## Key Principles

- **Always template from existing** - `cx <command> get <id> -o json > template.json` before any create
- **Verify after create** - query logs/metrics to confirm the pipeline change took effect
- **Use `-o json`** - all payload inspection and creation should use JSON output
- **Check limits first** - `cx parsing-rules usage-limits` and `cx e2m limits` before creating to avoid hitting caps
- **Bulk operations** - use `cx parsing-rules bulk-delete --ids` for cleanup, not individual deletes

---

## Additional Resources

### Reference Files

- **[`references/e2m-schemas.md`](references/e2m-schemas.md)** - Complete Events2Metrics JSON wire format: `type`/`aggType` enum values, `logsQuery`/`spansQuery` filters, metric labels & fields, the TCO-tier compute model, cardinality/permutations sizing, and gotchas

---

## Related Skills

- **`cx-telemetry-querying`** - discover what data is available before configuring pipeline, and verify parsing results, enriched fields, and E2M metric series via log/metrics queries
- **`cx-cost-optimization`** - find high-volume High-tier sources worth converting to metrics, and move the raw logs High→Medium (TCO) after the E2M is verified
