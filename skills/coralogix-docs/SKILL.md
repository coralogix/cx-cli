---
name: coralogix-docs
description: |
  Search and read official Coralogix platform documentation using **`cx docs search`** and **`cx docs fetch`**.
  Use when the user asks how Coralogix features work, how to configure or use the UI, set up integrations
  (OpenTelemetry, agents, collectors, webhooks), manage API keys, explore spans/traces/logs in the product,
  configure alerts/SLOs/dashboards, or needs authoritative product docs — not live tenant telemetry.
metadata:
  version: "0.1.0"
---

# Coralogix docs (`cx docs search`, `cx docs fetch`)

These commands read **official Coralogix product documentation** from [coralogix.com/docs](https://coralogix.com/docs). They answer **how the platform works** and **how to configure or use it**. They do **not** query your tenant's logs, spans, metrics, or alerts.

No Coralogix API key is required.

## CLI commands

| Command | Purpose |
|---------|---------|
| `cx docs search <query>` | Find doc pages by keyword. Returns numbered titles + path suffixes. |
| `cx docs fetch <suffix>` | Download one page as markdown. Pass the suffix from search. |

### Flags

| Flag | Commands | Description |
|------|----------|-------------|
| `--limit` | `search` | Max results, 1–20 (default 5) |
| `-o json` / `-o agents` | both | Machine-readable output |

### Examples

```bash
cx docs search "explore spans" --limit 5
cx docs search "OpenTelemetry traces"
cx docs fetch user-guides/data_exploration/spans/
cx docs search "API keys" -o json
```

## When to use these commands

Use **`cx docs search`** + **`cx docs fetch`** when the user wants:

| Topic | Example questions |
|-------|-------------------|
| **UI / workflows** | "How do I view spans in Coralogix?", "Where is Explore spans?" |
| **Ingestion & integrations** | OpenTelemetry setup, agent/collector config, Send Your Data API keys |
| **Platform features** | Alerts, SLOs, dashboards, enrichments, parsing rules, retention |
| **Concepts & architecture** | How tracing works, trace-log correlation, data model |
| **Account & access** | API keys, SSO, roles, regions |

**Prefer these commands over guessing** when the answer depends on current Coralogix product behavior or UI navigation.

## When **not** to use these commands

| User need | Use instead |
|-----------|-------------|
| **Query live logs** | `cx logs` — [cx-telemetry-querying](../cx-telemetry-querying/SKILL.md) |
| **Query live spans/traces** | `cx spans` — [cx-telemetry-querying](../cx-telemetry-querying/SKILL.md) |
| **DataPrime syntax / commands** | `cx dataprime list` / `cx dataprime show` — [cx-telemetry-querying](../cx-telemetry-querying/SKILL.md) |
| **Alerts & incidents in the tenant** | `cx alerts`, `cx incidents` — [cx-alerts](../cx-alerts/SKILL.md), [cx-incident-management](../cx-incident-management/SKILL.md) |
| **Metrics (PromQL)** | `cx metrics` — [cx-telemetry-querying](../cx-telemetry-querying/SKILL.md) |
| **Discover field paths in tenant data** | `cx search-fields` |

## Standard workflow

1. **`cx docs search`** with a focused query (2–4 keywords, not full sentences).
2. Pick the most relevant **suffix** from the results.
3. **`cx docs fetch`** on that suffix.
4. Answer from the fetched content. Fetch additional pages only when needed.

```
User: "How do I show spans in the Coralogix website?"

1. cx docs search "explore spans" --limit 5
2. cx docs fetch user-guides/data_exploration/spans/
3. Summarize: Explore → spans dataset → Spans/Traces/Flows tabs → drilldown
```

## Tips

### `cx docs search`

- Start with **2–4 focused terms**, not full sentences.
- If no matches, try synonyms or broader terms (`"tracing"` instead of `"distributed trace waterfall view"`).
- Increase `--limit` when the first page of hits is ambiguous.
- Results use **path suffixes** under `/docs/` (not full URLs) — pass them directly to **`cx docs fetch`**.

### `cx docs fetch`

- Pass the **suffix** from **`cx docs search`** (e.g. `user-guides/data_exploration/spans/`). Full URLs are rejected.
- **`cx docs fetch` one page at a time** — pick the best match first.

## Troubleshooting

| Problem | Action |
|---------|--------|
| **No search matches** | Broaden or rephrase the query; try feature name + category (`"spans UI"`, `"OTel ingestion"`). |
| **Fetched page is too narrow** | Search for the parent topic or run a second search with related terms. |
| **User wants their actual data** | Switch to `cx logs`, `cx spans`, `cx metrics`, or alerts — docs describe the product, not tenant contents. |

## Related

- **Telemetry queries:** [cx-telemetry-querying](../cx-telemetry-querying/SKILL.md)
- **Alerts:** [cx-alerts](../cx-alerts/SKILL.md)
- **Dashboards:** [cx-dashboards](../cx-dashboards/SKILL.md)
