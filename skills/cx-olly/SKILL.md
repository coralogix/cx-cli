---
name: cx-olly
description: This skill should be used when the user asks to "chat with AI", "ask Olly", "ask the agent", "send message to AI", "continue a chat", "follow up on chat", "get artifact", "download artifact", "list artifacts", "retrieve generated content", "AI-generated charts", "AI analysis", "conversational observability", "natural language query", or wants to interact with the Coralogix Observability Agent (Olly) using the cx CLI.
metadata:
  version: "0.1.0"
---

# Olly Observability Agent Skill

Use this skill to interact with Coralogix's Observability Agent (Olly) via the `cx olly` CLI commands. Olly can analyze your observability data, answer questions about alerts, metrics, logs, and generate artifacts like charts and reports.

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx olly ask "message"` | Send a message to the Observability Agent | `--chat-id`, `--model`, `--timeout` |
| `cx olly artifacts list` | List all generated artifacts | - |
| `cx olly artifacts get <id>` | Get artifact content by ID | - |

**Output format:** append `-o json` or `-o agents` for machine-readable output.

**Single-profile only:** `cx olly` commands do not support multi-profile fan-out. Use `-p <profile>` to specify a single profile.

## Chat Commands

### Start a new conversation

```bash
cx olly ask "What alerts fired today?"
```

This creates a new chat and returns a response along with a **Chat ID** that you can use for follow-up questions.

### Continue an existing chat

```bash
cx olly ask "Tell me more about the error rates" --chat-id <chat-id>
```

Use `--chat-id` to continue a conversation and maintain context from previous messages.

### Model selection

Available models include `gpt-5.2` (default), `claude-sonnet-4-5`, `sonnet-4.6`, `gpt-5.4`, `claude-haiku-4-5`.

```bash
cx olly ask "Explain this error" --model claude-sonnet-4-5
```

### Timeout

For complex queries that may take longer, increase the timeout (default: 900 seconds):

```bash
cx olly ask "Deep analysis of last week's incidents" --timeout 1800
```

## Artifacts

Olly can generate artifacts like charts, tables, and reports. Artifact IDs appear as links in the agent's response text.

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
# Start investigation
cx olly ask "Why is the checkout service showing high latency?"

# Follow up with the chat ID from the response
cx olly ask "What changed in the last hour?" --chat-id abc-123-def

# Get any generated charts
cx olly artifacts list -o json | jq '.[0].id'
cx olly artifacts get <artifact-id>
```

### Get JSON output for scripting

```bash
# Get response as JSON
cx olly ask "List top 5 error messages" -o json | jq '.response'

# Parse artifacts
cx olly artifacts list -o json | jq '.[] | {id, filename, created_at}'
```

### Detailed analysis with specific model

```bash
cx olly ask "Perform root cause analysis for the outage on 2024-01-15" \
  --model claude-sonnet-4-5 \
  --timeout 1800
```

## Key Principles

- **Chat IDs enable context** - save the Chat ID from responses to continue conversations
- **Use `-o json` for scripting** - pipe to `jq` for filtering and extraction
- **Artifact IDs are in response text** - look for markdown links like `[Chart](https://...artifact_view/<id>)`
- **Single-profile only** - `cx olly` does not support multi-profile queries
- **Large artifacts auto-spill** - JSON content over the configured limit is saved to temp files

## Related Skills

- **`cx-telemetry-querying`** - for direct DataPrime/PromQL queries without AI agent assistance (covers logs, spans, metrics, RUM)
- **`cx-alerts`** - for managing alert definitions
