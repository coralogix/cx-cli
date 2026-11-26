---
name: cx-olly
description: This skill should be used when the user asks to "chat with AI", "ask Olly", "ask the agent", "send message to AI", "continue a chat", "follow up on chat", "get artifact", "download artifact", "list artifacts", "retrieve generated content", "AI-generated charts", "AI analysis", "conversational observability", "natural language query", "Olly rules", "list Olly rules", "add a rule for Olly", "team rules", "custom instructions for Olly", or wants to interact with the Coralogix Observability Agent (Olly) using the cx CLI.
metadata:
  version: "0.1.0"
---

# Olly Observability Agent Skill

Use this skill to interact with Coralogix's Observability Agent (Olly) via the `cx olly` CLI commands. Olly can analyze your observability data, answer questions about alerts, metrics, logs, and generate artifacts like charts and reports.

`cx olly ask` defaults `--agent-to-agent-mode` to **false**. **If you're an LLM/agent, pass `--agent-to-agent-mode`** - see "Agent-to-agent mode" below.

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx olly ask "message"` | Send a message to the Observability Agent | `--chat-id`, `--model`, `--timeout`, `--agent-to-agent-mode` |
| `cx olly artifacts list` | List generated artifacts, newest first | `--chat-id`, `--type`, `--start`/`--end` (creation time), `--limit`, `--offset` |
| `cx olly artifacts get <id>` | Get artifact content by ID | - |
| `cx olly rules list` | List rules | `--team` |
| `cx olly rules get <rule-id>` | Show one rule with its full text | `--team` |
| `cx olly rules create --rule <text>` | Create a rule | `--team`, `--yes` |
| `cx olly rules update <rule-id> --rule <text>` | Replace a rule's text | `--team`, `--yes` |
| `cx olly rules enable <rule-id>` / `disable <rule-id>` | Turn a rule on or off | `--team`, `--yes` |
| `cx olly rules delete <rule-id>` | Delete a rule | `--team`, `--yes` |

**Output format:** append `-o json` or `-o toon` for machine-readable output.

**Single-profile only:** `cx olly` commands do not support multi-profile fan-out. Use `-p <profile>` to specify a single profile.

## Running Olly (async by default)

**Run `cx olly ask` asynchronously by default:** launch it as a **background process and poll it for completion** rather than blocking on it. Olly investigations routinely take minutes, so this is the normal mode for `cx olly ask`.

Only run `cx olly ask` inline (foreground, blocking) for a short question you expect Olly to answer quickly — a quick lookup or a one-line follow-up. When in doubt, run it in the background.

## Chat Commands

### Start a new conversation

```bash
cx olly ask "What alerts fired today?" --agent-to-agent-mode
```

This creates a new chat and returns a response along with a **Chat ID** that you can use for follow-up questions.
Remove `--agent-to-agent-mode` if you don't have context to share (like quick access to source files) or if the created chat is only for human usage.

### Continue an existing chat

```bash
cx olly ask "Tell me more about the error rates" --chat-id <chat-id> --agent-to-agent-mode
```

Use `--chat-id` to continue a conversation and maintain context from previous messages. Background the follow-up too when it kicks off another investigation.

### Model selection

Available models include `gpt-5.2` (default), `claude-sonnet-4-5`, `sonnet-4.6`, `gpt-5.4`, `claude-haiku-4-5`.

```bash
cx olly ask "Explain this error" --model claude-sonnet-4-5 --agent-to-agent-mode
```

### Timeout

For complex queries that may take longer, increase Olly's response timeout
(default: 900 seconds):

```bash
cx olly ask "Deep analysis of last week's incidents" --timeout 1800 --agent-to-agent-mode
```

`--http-timeout <SECONDS>` (or `CX_HTTP_TIMEOUT`) sets the HTTP request
deadline for all CLI commands, including Olly. It is separate from the Olly
response timeout.

### Agent-to-agent mode

`--agent-to-agent-mode` defaults to `false`, since `cx olly ask` is used directly by humans as well as by agents. **If you're an LLM/agent, pass `--agent-to-agent-mode`** to opt into shorter, sub-agent-style responses: no charts/tables, clarifying questions instead of guessing, and reliance on your broader context.

```bash
cx olly ask "Analyze this for me" --agent-to-agent-mode
```

**It's per-call, not per-chat.** `--chat-id` does not remember it - re-pass `--agent-to-agent-mode` on every follow-up turn, or the mode silently flips back to human-facing mid-conversation.

## Artifacts

Olly can generate artifacts like query results, previews, and citations. Artifact IDs appear as links in the agent's response text.

### List artifacts

```bash
cx olly artifacts list
cx olly artifacts list -o json
cx olly artifacts list --chat-id <chat-id>
cx olly artifacts list --type coralogix_metrics --start now-1d
cx olly artifacts list --limit 5 --offset 5
```

With no flags it returns the newest 100 artifacts across all chats. Narrow it with:

| Flag | Meaning |
|---|---|
| `--chat-id <id>` | Only artifacts from one chat (the chat ID that `cx olly ask` prints) |
| `--type <type>` | One of `coralogix_dataprime`, `coralogix_metrics`, `kubernetes_query_results`, `alert_watch_data`, `github_file_content`, `sandbox_generated_file`, `schema_health_suggestions` |
| `--start <time>` / `--end <time>` | Creation time window, inclusive. ISO 8601 or relative (`now-1h`) |
| `--limit <n>` / `--offset <n>` | Paging. `--limit 5 --offset 5` is the second page of five |

Each row has `id`, `type`, `chat_id`, `chat_title`, `chat_type`, `interaction_id` and `created_at`. The list carries no content. Use `artifacts get <id>` for the content.

### Get artifact content

```bash
cx olly artifacts get <artifact-id>
cx olly artifacts get <artifact-id> -o json
```

The `artifacts get` command automatically:
1. Fetches artifact metadata
2. Downloads content from the presigned URL
3. **Decompresses gzip** content
4. **Parses JSON** and uses spill logic for large content
5. Saves non-JSON text to a temp file

Output behavior:
- **JSON content**: Displayed directly, or spilled to file if large
- **Text content**: Saved to temp file (e.g., `/tmp/cx_results_artifact_<id>_<hash>.txt`)

## Rules

Rules are instructions Olly follows when it answers. Personal rules apply to your own chats. Team rules apply to everyone on the team. Without `--team` the commands act on personal rules; with `--team` they act on team rules.

### List and read rules

```bash
cx olly rules list
cx olly rules list --team -o json
cx olly rules get <rule-id>
cx olly rules get <rule-id> --team -o json
```

A rule has `id`, `rule` (the text), `enabled` and `created_at`. The `list` table cuts long rule text; `-o json` and `rules get` show the full text.

### Create and change rules

```bash
cx olly rules create --rule "Answer in English" --yes
cx olly rules create --rule "Always link the runbook" --team --yes
cat rule.txt | cx olly rules create --rule - --yes
cx olly rules update <rule-id> --rule "Answer in English, briefly" --yes
cx olly rules disable <rule-id> --team --yes
cx olly rules enable <rule-id> --team --yes
cx olly rules delete <rule-id> --yes
```

- `--rule -` reads the rule text from stdin.
- `create`, `update`, `enable`, `disable` and `delete` ask for confirmation. In agent mode they fail without `--yes`: ask the user to confirm, then re-run with `--yes`. `--read-only` blocks all five.
- `create` and `update` print the rule with `-o json`, so the new rule's `id` can be read from the output.
- **422** means Olly rejected the rule text (it looks like a prompt injection or contains foul language). The error shows Olly's reason.
- **403 on personal rules** usually means the profile uses a team API key. Personal rules need a personal API key or a logged-in profile. Team rules work with any API key that has the permissions below.
- **Team rule permissions:** reading needs `olly-team-rules:Read` or `olly-team-rules:Manage`; writing needs `olly-team-rules:Manage`.
- **Disabled team rules** are visible only with `olly-team-rules:Manage`. Without it, `list` leaves them out and `get` returns 404.

## Workflow Examples

### Investigate an issue

```bash
# Start investigation — run in the background and poll for completion
cx olly ask "Why is the checkout service showing high latency? Check logs with 'checkout:' strings and aws related metrics" --agent-to-agent-mode

# Follow up with the chat ID from the response — also background and poll
cx olly ask "What changed in the last hour?" --chat-id 3f2b8c1e-7a4d-4e9b-9c2a-1d5e6f7a8b9c --agent-to-agent-mode

# Once the interaction has completed, get any generated charts
cx olly artifacts list --chat-id 3f2b8c1e-7a4d-4e9b-9c2a-1d5e6f7a8b9c -o json | jq -r '.[].id'
cx olly artifacts get <artifact-id>
```

### Get JSON output for scripting

```bash
# Get response as JSON
cx olly ask "List top 5 error messages" -o json --agent-to-agent-mode | jq '.response'

# Parse artifacts
cx olly artifacts list -o json | jq '.[] | {id, type, chat_title, created_at}'
```

### Detailed analysis with specific model

```bash
cx olly ask "Perform root cause analysis for the outage on 2024-01-15" \
  --model claude-sonnet-4-5 \
  --timeout 1800 \
  --agent-to-agent-mode
```

## Key Principles

- **Async by default** - run `cx olly ask` as a background process and poll it for completion; only run inline (blocking) for short questions you expect Olly to answer quickly
- **Chat IDs enable context** - save the Chat ID from responses to continue conversations
- **Use `-o json` for scripting** - pipe to `jq` for filtering and extraction
- **Artifact IDs are in response text** - look for markdown links like `[Chart](https://...artifact_view/<id>)`
- **Single-profile only** - `cx olly` does not support multi-profile queries
- **Large artifacts auto-spill** - JSON content over the configured limit is saved to temp files
- **Check source code before asking** - give Olly concrete context from the source code if available, such as which metric to start investigating from, before calling it
- **Limit investigation scope** - guide Olly to the correct limited scope, for example limit to just logs or to specific time ranges
- **Pass `--agent-to-agent-mode` when calling as an LLM/agent** - it defaults to `false` (human-facing); agents should opt in for shorter, sub-agent-style responses

## Related Skills

- **`cx-telemetry-querying`** - for direct DataPrime/PromQL queries without AI agent assistance (covers logs, spans, metrics, RUM)
- **`cx-alerts`** - for managing alert definitions
