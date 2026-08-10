---
name: cx-olly
description: This skill should be used when the user asks to "chat with AI", "ask Olly", "ask the agent", "send message to AI", "continue a chat", "follow up on chat", "get artifact", "download artifact", "list artifacts", "retrieve generated content", "AI-generated charts", "AI analysis", "conversational observability", "natural language query", "run olly in background", "poll a long-running olly investigation", or wants to interact with the Coralogix Observability Agent (Olly) using the cx CLI.
metadata:
  version: "0.1.0"
---

# Olly Observability Agent Skill

Use this skill to interact with Coralogix's Observability Agent (Olly) via the `cx olly` CLI commands. Olly can analyze your observability data, answer questions about alerts, metrics, logs, and generate artifacts like charts and reports.

`cx olly ask` blocks until Olly finishes and can take many minutes. **If you're an agent, launch it in the background and poll instead of blocking your turn** - see "Run in background".

## CLI Commands

| Command | Purpose | Key flags |
|---|---|---|
| `cx olly ask "message"` | Send a message to the Observability Agent (blocking - background it yourself for long queries) | `--chat-id`, `--model`, `--timeout` |
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

`--timeout` (default: 900 seconds) is the server-side cap on how long the backend will keep working before it gives up. Raise it for complex queries:

```bash
cx olly ask "Deep analysis of last week's incidents" --timeout 1800
```

Raising `--timeout` is complementary to - not a substitute for - backgrounding the call. It bounds how long the underlying request runs; it does not stop `cx olly ask` from blocking your terminal/turn while it runs. See "Run in background" below.

### Run in background (preferred for long queries)

There is no `--background` flag and no CLI subcommand to poll an in-flight interaction - `cx olly ask` always sends `should_block: true` to the backend and blocks until it returns. For any query that might take more than a few seconds, **background the process yourself** instead of waiting on it synchronously.

**Preferred - use your own background-execution capability** (e.g. a coding agent's background task tool) to run `cx olly ask` and poll its output, so your turn/session isn't blocked. Do not sit in a long blocking `sleep` waiting for it.

**Plain shell fallback**, redirecting stdout/stderr so progress messages (written to stderr) don't corrupt the JSON result (written to stdout):

```bash
cx olly ask "Perform root cause analysis for the outage on 2024-01-15" \
  --timeout 1800 -o json > /tmp/olly_rca.json 2> /tmp/olly_rca.log &
OLLY_PID=$!

# Poll every ~15-30s; do other work between checks instead of tight-looping
kill -0 "$OLLY_PID" 2>/dev/null && echo "still running"

# Once the process has exited, read the result
jq -r '.[0].status, .[0].chat_id, .[0].response' /tmp/olly_rca.json
```

Use `nohup ... & disown` if the shell/session might close before the command finishes.

**Parsing the result:** `-o json` prints a single-element array (`.[0].chat_id`, `.[0].interaction_id`, `.[0].status`, `.[0].response`); `-o agents` prints TOON, not JSON - use `-o json` for scripted polling.

**Caveat:** because the request blocks, `chat_id` is only available once the command finishes - you can't send a follow-up mid-flight. Wait for the backgrounded job to finish before using `--chat-id`.

### Interaction status

| Status | Meaning | Action |
|--------|---------|--------|
| `completed` | Success, response available | Use `.[0].response` |
| `in_progress` | Server hit `timeout_seconds` before finishing; `response` may be absent (text output prints `No response received.`) | Re-ask the same `--chat-id` with a larger `--timeout`, or check `cx olly artifacts list` for partial output |
| `error` | Failed | Read the error message, retry with a narrower/more concrete query |
| `stopped` | Cancelled/interrupted | Retry if needed |

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
# Start investigation (background deep investigations - see "Run in background")
cx olly ask "Why is the checkout service showing high latency?"

# Follow up with the chat ID from the response
cx olly ask "What changed in the last hour?" --chat-id abc-123-def

# Get any generated charts
cx olly artifacts list -o json | jq '.[0].id'
cx olly artifacts get <artifact-id>
```

### Get JSON output for scripting

```bash
# Get response as JSON (result is a single-element array)
cx olly ask "List top 5 error messages" -o json | jq -r '.[0].response'

# Parse artifacts
cx olly artifacts list -o json | jq '.[] | {id, filename, created_at}'
```

### Detailed analysis with specific model (backgrounded)

```bash
cx olly ask "Perform root cause analysis for the outage on 2024-01-15" \
  --model claude-sonnet-4-5 \
  --timeout 1800 -o json > /tmp/olly_rca.json 2> /tmp/olly_rca.log &
OLLY_PID=$!

# ... poll $OLLY_PID / do other work, then:
jq -r '.[0].response' /tmp/olly_rca.json
```

## Key Principles

- **Chat IDs enable context** - save the Chat ID from responses to continue conversations
- **Use `-o json` for scripting** - pipe to `jq` for filtering and extraction
- **Artifact IDs are in response text** - look for markdown links like `[Chart](https://...artifact_view/<id>)`
- **Single-profile only** - `cx olly` does not support multi-profile queries
- **Large artifacts auto-spill** - JSON content over the configured limit is saved to temp files
- **Background long asks** - `cx olly ask` blocks (no `--background` flag); launch it as a background job and poll for completion instead of blocking your turn

## Related Skills

- **`cx-telemetry-querying`** - for direct DataPrime/PromQL queries without AI agent assistance (covers logs, spans, metrics, RUM)
- **`cx-alerts`** - for managing alert definitions
