---
name: cx-ai-center
description: >
  Use this skill for any question or action about the user's AI/GenAI applications or agents —
  their behavior, prompts/responses, quality, hallucinations, guardrails, security, cost/tokens,
  errors, evaluations/policies, model pricing, or configuration — including comparing or tracking
  agents over time. It covers both analyzing AI telemetry (GenAI spans) and managing AI Center
  config via the `cx ai-center` commands.
metadata:
  version: "0.1.0"
---

# AI Center Skill

**This is the tool for anything about AI/GenAI applications** — both **questions** about their
behavior (prompts/responses, quality, hallucinations, guardrails, security, cost/tokens, errors,
latency — everything AI apps expose through their GenAI spans/tags) and **actions** to manage them
(applications, evaluations/policies, policy↔app links, model pricing). If a request touches an AI
application or its GenAI telemetry, use this skill.

Coralogix **AI Center** observes, evaluates, and guards GenAI/LLM applications. This skill
answers questions about AI apps from two sources:

- **Configuration** (this skill's `cx ai-center` commands): the AI application inventory,
  configured evaluations/policies, coverage, custom evaluations, and model pricing — none of
  which live in span telemetry.
- **Telemetry** (GenAI spans): what users asked, how the model answered, cost, tokens,
  latency, errors, tool calls, and eval/guardrail verdicts — queried with **`cx spans '<DataPrime>'`**.
  See [references/ai-center-queries.md](references/ai-center-queries.md) for the full,
  runnable query library, span schema, and playbooks.

Match the source to the question: *"which apps lack guardrails"* → config
(`cx ai-center applications list`); *"what are users asking my chatbot"* → telemetry
(`cx spans '…'`, reading the conversation from the GenAI spans). Some questions need **both** —
e.g. *"is my chatbot's PII policy actually catching PII?"* joins config (is the policy enabled)
with telemetry (the PII verdicts + the messages).

---

## Destructive Operation Safety

All write operations (`create`, `update`, `delete`, `add`, `remove`, `set`)
require interactive confirmation. `ai-center` is a **risky** command, so writes are also
gated by `allow_risky_commands` in `~/.cx/config.toml`. To skip the prompt in scripts, pass
`--yes`.

**IMPORTANT: NEVER pass `--yes` without explicit user approval.** Before executing any write:
1. Describe the exact operation to the user (what will be created/modified/deleted/linked).
2. Wait for the user to confirm.
3. Only then execute with `--yes`.

Read operations (`list`, `get`, `coverage`, `list-for-application`, `model-pricing get`) do
not require confirmation and can be run freely.

### Read-Only Mode
Use `--read-only` (or `CX_READ_ONLY=1`) to block every write at the CLI level — safe for
exploration.

### Agent Mode
When running inside an AI agent (Claude Code, Cursor, Codex, …), cx detects it and — instead of
showing a confirmation prompt that would hang forever (no human is there to type y/n) — stops
immediately with an error telling you to get the user's approval, then re-run with `--yes`.

### No delete commands (by design)
The CLI intentionally exposes **no delete** for custom-evaluation policies, AI applications, or
model pricing — even though the AI v3 API has those delete endpoints, `cx ai-center` does not
surface them.
- **Custom-evaluation policy:** can't be deleted; to take it off an app, detach with
  `custom-evaluations remove` (the policy object survives and can be re-attached).
- **Model pricing:** no delete command. It's **team-wide** (not per-app), so to change or clear
  it, run `model-pricing set` with a new map (an empty map `{}` clears all overrides) — `set`
  replaces the whole set.

---

## Golden rule

For **content** questions (quality, hallucination, sentiment, topics) read the actual
conversation and cite the `traceID` — don't rely on verdict tags alone. The transcript lives in
one of two conventions (`gen_ai.input.messages`/`output.messages`, or the older indexed
`gen_ai.prompt.<n>`/`completion.<n>` tags); read it with the **Reading conversations (content
questions)** queries in the library, which handle both and exclude the system prompt and tool
traffic. Full guidance:
[references/ai-center-queries.md](references/ai-center-queries.md).

---

## CLI Commands

**Show names to the user; use UUIDs only internally.** When presenting results, refer to apps
and evaluations by their human names (application/subsystem, evaluation name), not raw UUIDs.
The UUID is only needed to *call* a by-id or write command — resolve it yourself from the
matching `list` command (never guess or make the user paste a UUID).

### Applications (inventory + guarded status)

| Command | Purpose |
|---------|---------|
| `cx ai-center applications list` | List AI apps incl. `guardrailsIntegrated` (guarded) status |
| `cx ai-center applications list --evaluation-type <TYPE>` | Filter to apps using an eval type (repeatable) |
| `cx ai-center applications list --page-size <N> --page-offset <N>` | Paginate |
| `cx ai-center applications get <application-id>` | One application by UUID |

### Evaluations (configured policies on apps)

| Command | Purpose |
|---------|---------|
| `cx ai-center evaluations list` | All configured evaluations |
| `cx ai-center evaluations list --application <app> --subsystem <sub>` | Scope to one app (the pair) |
| `cx ai-center evaluations list --evaluation-type <TYPE>` | Filter by type — `<TYPE>` is the API enum (e.g. `PII`, `TOXICITY`, `PROMPT_INJECTION`; the keys from `coverage`), **not** the lowercase form |
| `cx ai-center evaluations get <evaluation-id>` | One evaluation by UUID |
| `cx ai-center evaluations create --from-file eval.json` | Create/enable an evaluation *(write)* |
| `cx ai-center evaluations update <evaluation-id> --from-file patch.json` | Partial update *(write)* |
| `cx ai-center evaluations delete <evaluation-id>` | Remove an evaluation from its app *(write)* |

### Custom evaluations (policies) & application links

| Command | Purpose |
|---------|---------|
| `cx ai-center custom-evaluations list` | All custom evaluation policies |
| `cx ai-center custom-evaluations list-for-application <application-id>` | Policies linked to one app |
| `cx ai-center custom-evaluations create --from-file policy.json` | Create a custom policy *(write)* |
| `cx ai-center custom-evaluations update <id> --from-file patch.json` | Partial update *(write)* |
| `cx ai-center custom-evaluations add <evaluation-id> <application-id>` | Attach a policy to an app *(write)* |
| `cx ai-center custom-evaluations remove <evaluation-id> <application-id>` | Detach (reversible) *(write)* |

### Coverage & model pricing

| Command | Purpose |
|---------|---------|
| `cx ai-center coverage` | Map of each evaluation type → number of apps using it (coverage / gap analysis) |
| `cx ai-center model-pricing get` | Team's custom per-model pricing overrides |
| `cx ai-center model-pricing set --from-file prices.json` | Set team pricing (team-wide, new data only) *(write)* |

The `--from-file` bodies for `evaluations` and `custom-evaluations` match the AI v3 API
shape verbatim; use `-` to read JSON from stdin. **Exception:** `model-pricing set` takes just
the raw `model→price` map — cx wraps it as `{"prices": …}` for you, so do **not** include the
outer `prices` envelope. Each model maps to a price object; all four fields are optional doubles
(USD per **one million** tokens), omit the ones that don't apply:

```json
{
  "gpt-4o": {
    "inputPricePerMillionTokens": 2.5,
    "outputPricePerMillionTokens": 10,
    "cacheReadPricePerMillionTokens": 1.25,
    "cacheWritePricePerMillionTokens": 3.75
  }
}
```

An empty map `{}` clears all overrides (set replaces the whole set — it's team-wide, new data only).

---

## Common workflows

### Inventory & guardrail gaps
```bash
# Which apps are NOT guarded?
cx ai-center applications list -o json | jq '[.[] | select(.guardrailsIntegrated==false)]'
```

### Enable a policy on an app (write — confirm first!)
```bash
# 1. Describe to the user; 2. get approval; 3. then:
cx ai-center evaluations create --from-file eval.json --yes
# eval.json: { "application": "...", "subsystem": "...", "config": { "<type>": {...} }, "isEnabled": true }
```

### Read the actual conversations (telemetry, not config)
Use `cx spans` with the query library in
[references/ai-center-queries.md](references/ai-center-queries.md) — reading messages, cost,
latency, errors, tool calls, and per-user analysis.

---

## Key principles

- **Config vs. telemetry:** inventory / evaluations / policies / coverage / pricing → `cx ai-center`;
  content / cost / latency / errors / verdicts → GenAI spans via `cx spans`. Don't answer one
  from the other.
- **Confirm before writes.** Describe the operation, get approval, then run with `--yes`.

---

## Related Skills

- `cx-telemetry-querying` — general logs/spans/metrics/DataPrime querying (the engine behind
  the `cx spans` queries used here).
- `cx-olly` — the conversational AI assistant (`cx olly ask`).
