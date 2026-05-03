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
  or wants to configure how Coralogix processes, enriches, or transforms ingested data.
version: 0.1.0
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

Use the `cx-query-logs` skill to query recent logs and confirm fields are extracted:

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

### 3. Add Enrichment Rules

```bash
cx enrichments add --from-file enrichment-rules.json
```

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

### 1. Design the Metric

Decide the metric name, labels, and aggregation type before creating.

### 2. Check Limits

```bash
cx e2m limits -o json
cx e2m labels-cardinality -o json
```

### 3. Get a Template

```bash
cx e2m list -o json
cx e2m get <existing-e2m-id> -o json > e2m-template.json
```

### 4. Create E2M Definition

```bash
cx e2m create --from-file e2m-definition.json
```

### 5. Verify Metric

Use the `cx-metrics-query` skill to confirm the new metric appears:

```bash
cx metrics search --name "new_metric_name"
```

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

Use the `cx-metrics-query` skill to confirm the precomputed metric is available:

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

## Related Skills

- **`cx-query-logs`** - verify parsing results and enriched fields in log data
- **`cx-metrics-query`** - verify E2M and recording rule output metrics
- **`cx-dataprime`** - DataPrime syntax reference for rule expressions
- **`cx-telemetry-querying`** - discover what data is available before configuring pipeline
