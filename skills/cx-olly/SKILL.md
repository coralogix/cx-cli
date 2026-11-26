---
name: cx-olly
description: This skill should be used when the user asks to "chat with AI", "ask Olly", "ask the agent", "send message to AI", "continue a chat", "follow up on chat", "list chats", "find a chat ID", "show chat history", "which models does Olly support", "list Olly models", "get artifact", "download artifact", "list artifacts", "retrieve generated content", "AI-generated charts", "AI analysis", "conversational observability", "natural language query", or wants to interact with the Coralogix Observability Agent (Olly) using the cx CLI.
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
| `cx olly chats list` | List chats, most recently active first | `--type`, `--metadata`, `--limit`, `--offset` |
| `cx olly chats get <chat-id>` | Show a chat with its messages | - |
| `cx olly models` | List the model IDs `ask --model` accepts | - |
| `cx olly artifacts list` | List all generated artifacts | - |
| `cx olly artifacts get <id>` | Get artifact content by ID | - |

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

The default is `gpt-5.2`. The available models differ by region and change over time, so list them instead of guessing:

```bash
cx olly models
cx olly ask "Explain this error" --model <model-id> --agent-to-agent-mode
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

## Chats

### Find a chat

```bash
cx olly chats list
cx olly chats list --type ai_connector --limit 10
cx olly chats list --limit 50 --offset 50
cx olly chats list --metadata '{"scheduled_task_id":"<id>"}' -o json
```

Chats are listed most recently active first, 50 at a time by default (`--limit` 1-10000, `--offset` to page). Use the `id` with `cx olly ask --chat-id` or `cx olly chats get`.

- `--type` is one of `web`, `api`, `slack`, `teams`, `query_assistant`, `ai_connector`, `cases`, `scheduled`. Chats created by `cx olly ask` are `ai_connector`.
- `--metadata` takes a JSON object and returns only the chats whose metadata contains it. Invalid JSON fails before any request is sent.

### Read a chat

```bash
cx olly chats get <chat-id>
cx olly chats get <chat-id> -o json
```

Text mode prints the conversation in order (who said what). `-o json` prints the full API response, including `messages` and `interactions`. In text and toon mode a chat over the configured size limit is written to a temp file and the path is printed.

## Artifacts

Olly can generate artifacts like query results, previews, and citations. Artifact IDs appear as links in the agent's response text.

### List all artifacts

```bash
cx olly artifacts list
cx olly artifacts list -o json
```

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

## Workflow Examples

### Investigate an issue

```bash
# Start investigation — run in the background and poll for completion
cx olly ask "Why is the checkout service showing high latency? Check logs with 'checkout:' strings and aws related metrics" --agent-to-agent-mode

# Follow up with the chat ID from the response — also background and poll
cx olly ask "What changed in the last hour?" --chat-id abc-123-def --agent-to-agent-mode

# Once the interaction has completed, get any generated charts
cx olly artifacts list -o json | jq '.[0].id'
cx olly artifacts get <artifact-id>
```

### Get JSON output for scripting

```bash
# Get response as JSON
cx olly ask "List top 5 error messages" -o json --agent-to-agent-mode | jq '.response'

# Parse artifacts
cx olly artifacts list -o json | jq '.[] | {id, filename, created_at}'
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
- **Chat IDs enable context** - save the Chat ID from responses to continue conversations; `cx olly chats list` finds one you lost
- **Do not guess model IDs** - run `cx olly models`
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
