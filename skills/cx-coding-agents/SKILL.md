---
name: cx-coding-agents
description: |
  Use this skill when the user asks about AI Center Coding Agents data, wants to
  reproduce or extend the Coding Agents dashboards, or asks questions about usage,
  cost, tokens, sessions, tools, code impact, users, models, spans, or logs for
  Claude Code, Codex, Cursor, Gemini CLI, or Copilot CLI.
metadata:
  version: "1.0.0"
  internal: true
---

# Coding Agents Skill

Use this skill as the entry point for any investigation or data question about AI Center Coding Agents. It identifies which data source each agent uses and tells you which reference files to load before querying.

## Supported Agents

| Agent | Data Type | Source Identifier |
|---|---|---|
| Claude Code | Metrics | `claude_code_*` metric family |
| Claude Code (session text) | Logs | `source ai_sessions_claude` |
| Codex | Logs | `service.name == 'codex_cli_rs'` or `'codex-app-server'` |
| Codex (latency) | Spans | `$l.serviceName == 'codex_cli_rs'` or `'codex-app-server'` |
| Cursor | Spans | `$l.serviceName == 'cursor-agent'` |
| Gemini CLI | Metrics | `gemini_cli_*` metric family |
| Copilot CLI (direct OTel) | Spans | `$l.serviceName == 'github-copilot'` or `tags['otel.scope.name'] == 'github.copilot'` |
| GitHub Copilot Collector | Metrics | `github_copilot_*` metric family |

---

## Loading References

Load the agent reference first, then the shared query-language files it depends on:

| Agent | Agent Reference | Shared References |
|---|---|---|
| Claude Code (metrics) | `references/claude-code.md` | `references/promql-guidelines.md` + `references/metrics-querying.md` |
| Claude Code (sessions) | `references/claude-code.md` | `references/dataprime-reference.md` + `references/logs-querying.md` |
| Codex | `references/codex.md` | `references/dataprime-reference.md` + `references/logs-querying.md` + `references/spans-querying.md` |
| Cursor | `references/cursor.md` | `references/dataprime-reference.md` + `references/spans-querying.md` |
| Gemini CLI | `references/gemini-cli.md` | `references/promql-guidelines.md` + `references/metrics-querying.md` |
| Copilot CLI (direct OTel) | `references/copilot-cli.md` | `references/dataprime-reference.md` + `references/spans-querying.md` |
| GitHub Copilot Collector | `references/copilot-cli.md` | `references/promql-guidelines.md` + `references/metrics-querying.md` |
| Cross-agent comparison | All agent references | All shared references |

---

## Safety

All query commands (`cx metrics`, `cx logs`, `cx spans`, `cx dataprime`) are read-only. They never modify data and can be run freely without `--yes`.

---

## CLI Commands

| Command | Purpose | When to Use |
|---|---|---|
| `cx metrics search --name '<pattern>'` | Find metrics by name | Discover available `claude_code_*`, `gemini_cli_*`, `github_copilot_*` metrics |
| `cx metrics query '<expr>'` | Instant PromQL query | Point-in-time metric snapshot |
| `cx metrics query-range '<expr>'` | Range PromQL query (time series) | Claude Code, Gemini CLI, GitHub Copilot Collector trends |
| `cx logs '<dataprime_query>'` | Log query | Claude Code session text, Codex logs |
| `cx spans '<dataprime_query>'` | Span query | Cursor, Codex latency, Copilot CLI direct OTel |
| `cx dataprime query '<query>'` | Raw DataPrime query | When you need an explicit `source logs` or `source spans` in the query |

**Output format:** append `-o agents` to any command when passing results to an agent or further processing.

**Multi-profile:** use `-p <profile>` (repeatable) to fan out across profiles simultaneously.

---

## First Response

Identify the requested agent and analysis goal. If any required scope is missing, ask before querying:

- **Agent:** Claude Code, Codex, Cursor, Gemini CLI, or Copilot CLI.
- **Time range.**
- **Optional filters:** application, subsystem, user, model, session, repository.
- **Output shape:** quick answer, table, trend, top-N ranking, investigation notes, or query only.

> If the user says "GitHub Copilot Codex", clarify whether they mean Codex or Copilot CLI. AI Center has separate Codex and Copilot CLI dashboards.

Use the same data source as the corresponding dashboard whenever possible:

- **Claude Code metrics:** PromQL over `claude_code_*` metrics.
- **Claude Code session text:** DataPrime over `ai_sessions_claude` logs.
- **Codex:** DataPrime over Codex logs for tokens, sessions, models, users, and tools; spans only for `run_turn` latency.
- **Cursor:** DataPrime over Cursor spans. Cursor reports prompt length and file edits, not provider token counts.
- **Gemini CLI:** PromQL over `gemini_cli_*` metrics.
- **Copilot CLI direct OTel:** DataPrime over Copilot spans.
- **GitHub Copilot Collector:** PromQL over `github_copilot_*` org/user/billing metrics when that collector data is present.

---

## Time and Filters

### PromQL (Claude Code, Gemini CLI, GitHub Copilot Collector)

Use `increase(metric[<range>])` for counters over the selected window. Common label filter pattern:

```promql
{user_email="<user>",model="<model>",cx_application_name="<app>",cx_subsystem_name="<subsystem>"}
```

- Use `sum by (...)`, `count by (...)`, or `topk(N, ...)` for grouping.
- For chart trends, use an interval window based on the selected range rather than the full range.

### DataPrime (Codex, Cursor, Copilot CLI direct, Claude Code sessions)

- Add `| filter ...` clauses for user, model, application, subsystem, session, and operation.
- Prefer `groupby ... aggregate ...` for dashboard-like tables.
- For spans: app/subsystem labels are `$l.applicationName` and `$l.subsystemName` (mixed case).
- For logs: app/subsystem labels are `$l.applicationname` and `$l.subsystemname` (lowercase).

App/subsystem filter pattern:

```text
| filter $l.applicationName == '<application>' && $l.subsystemName == '<subsystem>'
```

---

## Cross-Agent Questions

When comparing agents:

- **Normalize units** before comparing: cost in USD, tokens as input/output/total, runtime in milliseconds, sessions as distinct conversations.
- **State which signals are not equivalent.** Cursor prompt length is not a provider token count. Direct Copilot CLI users are pseudonymous IDs (`enduser.pseudo.id`), not email addresses. GitHub Copilot Collector users may be logins, names, or emails.
- **Prefer per-agent sections** over a single merged table when dimensions differ.
- For "which agent is most used": compare sessions and users first, then tokens or span counts with caveats.
- For "which agent costs the most": Claude Code has first-class USD metrics. Copilot direct spans expose `github.copilot.cost`. Collector billing metrics expose net/gross/discount amounts by organization/SKU. Do not merge Copilot span cost and collector billing without explaining the source and unit difference.

---

## Answer Style

Always include:

- The agent and data source used.
- The query or a compact query excerpt.
- The time range and filters applied.
- A plain-language interpretation of the result.
- Caveats about empty data, approximate counts, pseudonymous users, missing labels, or non-comparable metrics.

Do not invent fields not listed in the agent reference files. If the user asks for data an agent does not emit, say what is available and propose the nearest supported query.
