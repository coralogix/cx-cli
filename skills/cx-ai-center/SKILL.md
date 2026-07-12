---
name: cx-ai-center
description: >
  Use this skill for ANY question or action about the user's AI/GenAI applications or agents —
  even when they don't say "AI"/"LLM" explicitly. Trigger on phrases like "which of my agents",
  "out of my LLM applications", "how is my agent doing", "is my agent behaving", "which agent
  deviates from its task", "compare the system prompt of my agent to a week ago", "compare two
  agents", "analyze AI interactions", "GenAI observability", "which AI apps do I have", "which
  apps lack guardrails", "AI evaluations", "AI policies", "custom evaluations", "evaluation
  coverage", "hallucinations", "prompt injection", "toxicity", "PII in prompts", "guardrails",
  "read the prompts/responses", "LLM cost and tokens", "model pricing overrides", "AI error
  rate", "agent tool calls", "user frustration/satisfaction with the AI", "topics users ask
  about", "session walkthrough", "AI Center" — or whenever the request concerns an AI
  application, agent, its prompts/responses, quality, safety, cost, or configuration, even if
  the words "AI"/"agent"/"LLM" are only implied.
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

A single question often spans both: e.g. *"which apps lack guardrails"* is config
(`cx ai-center applications list`), while *"what are users asking my chatbot"* is telemetry
(`cx spans '…'`, reading `gen_ai.input.messages`).

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

### What cannot be deleted
By design there is **no** command to delete a custom-evaluation policy, delete an AI
application, or delete model pricing. To "remove" a policy from an app, detach it with
`custom-evaluations remove` — the policy object itself survives and can be re-attached.
Model pricing is **team-wide** (not attached per app, so there's nothing to "detach"); to reset
or change it, run `model-pricing set` with a new map — it replaces the whole set.

---

## Golden rule

For **content** questions (quality, hallucination, sentiment, topics) read the actual
`gen_ai.input.messages` / `gen_ai.output.messages` and cite the `traceID` — don't rely on
verdict tags alone. Full guidance + the query library:
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
the raw `model→price` map (e.g. `{"gpt-4o": { ... }}`) — cx wraps it as `{"prices": …}` for you,
so do **not** include the outer `prices` envelope.

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
