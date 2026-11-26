---
name: cx-olly
description: This skill should be used when the user asks to "chat with AI", "ask Olly", "ask the agent", "send message to AI", "continue a chat", "follow up on chat", "schedule a prompt", "scheduled task", "run Olly every day", "watch a dashboard with Olly", "list scheduled tasks", "pause a scheduled task", "run a scheduled task now", "scheduled task run history", "get artifact", "download artifact", "list artifacts", "retrieve generated content", "AI-generated charts", "AI analysis", "conversational observability", "natural language query", or wants to interact with the Coralogix Observability Agent (Olly) using the cx CLI.
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
| `cx olly artifacts list` | List all generated artifacts | - |
| `cx olly artifacts get <id>` | Get artifact content by ID | - |
| `cx olly scheduled-tasks list` | List your scheduled tasks | `--kind`, `--shared`, `--dashboard` |
| `cx olly scheduled-tasks get <task-id>` | Show one task | - |
| `cx olly scheduled-tasks create` | Create a task from flags or a JSON file | `--title`, `--prompt`, `--model`, `--start-at`, `--every`, `--monthly`, `--from-file`, `--yes` |
| `cx olly scheduled-tasks update <task-id>` | Change title, prompt, model, reasoning effort or notification | `--title`, `--prompt`, `--model`, `--reasoning-effort`, `--from-file`, `--yes` |
| `cx olly scheduled-tasks set-schedule <task-id>` | Change when it runs | `--start-at`, `--every`, `--monthly`, `--yes` |
| `cx olly scheduled-tasks enable\|disable <task-id>` | Turn the schedule on or off | `--yes` |
| `cx olly scheduled-tasks share <task-id>` | Change who can see the task | `--with`, `--entity-id`, `--yes` |
| `cx olly scheduled-tasks delete <task-id>` | Delete the task | `--yes` |
| `cx olly scheduled-tasks run <task-id>` | Run it now | `--yes` |
| `cx olly scheduled-tasks runs <task-id>` | Show the run history, newest first | - |

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

## Scheduled tasks

A scheduled task is a prompt Olly runs on a schedule. It can watch a dashboard, and it can notify people about the result.

**Scheduled tasks need a personal API key or a logged-in profile.** A team API key gets a 403 on every `scheduled-tasks` command. Creating and running a task also needs AI consent to be on for the team.

Every command that changes something (`create`, `update`, `set-schedule`, `enable`, `disable`, `share`, `delete`, `run`) needs confirmation. As an agent, ask the user first, then re-run with `--yes`. `--read-only` blocks them.

### Find and read tasks

```bash
cx olly scheduled-tasks list                           # tasks you own
cx olly scheduled-tasks list --shared                  # tasks other users shared with you
cx olly scheduled-tasks list --kind watch_dashboard
cx olly scheduled-tasks list --dashboard <dashboard-id>
cx olly scheduled-tasks get <task-id>
cx olly scheduled-tasks get <task-id> -o json
```

`--kind` cannot be combined with `--dashboard`. A user can own a limited number of tasks; when `create` reports that the quota is reached, delete a task first.

### Create a task

A plain task needs only flags. `--model` takes the same model IDs as `cx olly ask --model`.

```bash
# Every day, first run in one hour
cx olly scheduled-tasks create --title "Daily errors" --prompt "Summarize new errors in the last 24h" \
    --model <model-id> --start-at now+1h --every 1d --yes

# Once, at a fixed time
cx olly scheduled-tasks create --title "Deploy check" --prompt "Did the 09:00 deploy raise errors?" \
    --model <model-id> --start-at 2026-12-01T09:30:00Z --yes
```

- `--start-at` is `now+<duration>` (`now+1h`, `now+3d`) or an ISO-8601 time with a timezone. It must be in the future and at most one month ahead.
- `--every` is a whole number of hours from `1h` to `1w` (`6h`, `1d`, `1w`). `--monthly` repeats every month. With neither, the task runs once.
- `--reasoning-effort` is `none`, `minimal`, `low`, `medium` (default), `high` or `xhigh`.

A task with a notification, a dashboard to watch, or sharing needs `--from-file` (`-` reads stdin). The file is the API's create body:

```json
{
  "title": "Checkout dashboard watch",
  "prompt": "Check the checkout dashboard for anomalies",
  "model_choice": "<model-id>",
  "reasoning_effort": "medium",
  "start_at": "2026-12-01T09:00:00Z",
  "interval_seconds": 86400,
  "enabled": true,
  "kind": "watch_dashboard",
  "metadata": {"dashboard_url": "https://<team>.app.coralogix.com/#/dashboards/<id>"},
  "notification": {
    "type": "on_condition",
    "condition": "error rate above 5%",
    "connector_ids": ["<connector-id>"]
  },
  "shared_options": {"shared_type": "entire_organization"}
}
```

```bash
cx olly scheduled-tasks create --from-file task.json --yes
```

- Only `title`, `prompt`, `model_choice` and `start_at` are required. A key the API does not read (a misspelled key, for example) fails the command before any request; the read-only keys of `get -o json` (`id`, `owner`, `next_run_at`, ...) are accepted.
- `interval_seconds` is a whole number of hours from 3600 to 604800. Use `"interval_months": 1` instead for monthly. Leave both out for a one-off.
- `kind` is `standard` (default; no `metadata`) or `watch_dashboard` (needs `metadata.dashboard_url`).
- `notification.type` is `every_run` (after every run), `olly_judges` (Olly decides whether the result is worth a notification) or `on_condition` (when `condition` is met; the only type that takes `condition`). `connector_ids` takes 1 to 5 email, Slack or Microsoft Teams connector IDs from `cx notifications connectors list`.
- `shared_options.shared_type` is `private` (default), `entire_organization`, or `specific_entities` with `shared_entity_ids`.

To copy a task, save it with `get -o json`, edit the file and create it again. Give it a new `start_at`: the saved one is usually in the past.

### Change a task

```bash
cx olly scheduled-tasks update <task-id> --title "Daily error summary" --yes
cx olly scheduled-tasks update <task-id> --model <model-id> --reasoning-effort high --yes
cx olly scheduled-tasks update <task-id> --from-file changes.json --yes   # e.g. {"notification": null}
cx olly scheduled-tasks set-schedule <task-id> --start-at now+1h --every 6h --yes
cx olly scheduled-tasks disable <task-id> --yes
cx olly scheduled-tasks enable <task-id> --yes
cx olly scheduled-tasks share <task-id> --with org --yes
cx olly scheduled-tasks delete <task-id> --yes
```

- `update` changes only the fields you pass. The notification can only be changed from a file; `"notification": null` stops notifications. An update file may hold `title`, `prompt`, `model_choice`, `reasoning_effort` and `notification`; any other key fails the command.
- `set-schedule` replaces the whole schedule: without `--every` or `--monthly` the task becomes a one-off. A disabled task stays disabled.
- `enable` and `disable` keep the schedule. A one-off task whose start time has passed cannot be enabled or disabled; give it a new time with `set-schedule` first.
- `share --with` takes `private`, `org` or `entities` (with `--entity-id`). Everyone the task is shared with can read the task, its run history and its run chats.
- Only the owner can change, run, share or delete a task. For a task that is only shared with you, these commands fail (the API answers 404).
- A title must be unique among your tasks, and one dashboard can have one watch task. A second one fails with "already exists".

### Run a task and read the result

```bash
cx olly scheduled-tasks run <task-id> --yes     # starts a run in the background
cx olly scheduled-tasks runs <task-id>          # status, times, error reason, chat ID, interaction ID
cx olly ask "Summarize this run" --chat-id <chat-id>   # ask about the run in its chat
```

`run` returns as soon as the run starts and prints the `runs` command to follow it. A manual run sends the task's notifications like a scheduled run. In `runs`, the status is `pending`, `running`, `succeeded`, `failed` or `stopped`; a failed run has an error reason (`quota_exceeded`, `ai_consent_denied`, `permission_denied`, `timeout`, `general_error`, `notification_verdict_not_reported`). Poll `runs` until the newest run is no longer `pending` or `running`, then read its chat. `runs -o json` also has `notification_verdict` and `notification_message`.

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
- **Chat IDs enable context** - save the Chat ID from responses to continue conversations
- **Use `-o json` for scripting** - pipe to `jq` for filtering and extraction
- **Artifact IDs are in response text** - look for markdown links like `[Chart](https://...artifact_view/<id>)`
- **Single-profile only** - `cx olly` does not support multi-profile queries
- **Scheduled tasks need a personal API key** - a team API key gets 403 on every `cx olly scheduled-tasks` command
- **Large artifacts auto-spill** - JSON content over the configured limit is saved to temp files
- **Check source code before asking** - give Olly concrete context from the source code if available, such as which metric to start investigating from, before calling it
- **Limit investigation scope** - guide Olly to the correct limited scope, for example limit to just logs or to specific time ranges
- **Pass `--agent-to-agent-mode` when calling as an LLM/agent** - it defaults to `false` (human-facing); agents should opt in for shorter, sub-agent-style responses

## Related Skills

- **`cx-telemetry-querying`** - for direct DataPrime/PromQL queries without AI agent assistance (covers logs, spans, metrics, RUM)
- **`cx-alerts`** - for managing alert definitions
