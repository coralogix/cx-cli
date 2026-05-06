# Training: Using the Coralogix CLI

This exercise introduces the `cx` CLI - the Coralogix terminal interface for querying logs, metrics, traces, alerts, and more. You'll practice using it directly as a human, through Claude Code skills, and finally in an Agent-to-Agent (A2A) flow via `cx olly`.

**Estimated time:** 60-90 minutes  
**Prerequisites:** `cx` installed and at least one profile configured (`cx profiles list`)

---

## Part 1: CLI Basics (Human Mode)

### 1.1 Orient yourself

```bash
# See all available commands
cx --help

# Dump the full command tree as structured JSON (useful for automation)
cx schema
```

**Exercise:** How many top-level command groups does `cx` have? Find the group that contains `tco` and `retentions`.

---

### 1.2 Query logs

```bash
# Basic log query - last 15 minutes, up to 100 results
cx logs query "source logs | limit 100" --since now-15m --until now

# Filter by severity
cx logs query "source logs | filter $severity == 'ERROR' | limit 50" --since now-1h --until now

# JSON output (good for piping to jq)
cx logs query "source logs | limit 10" --since now-15m -o json

# Agents output (compact, AI-optimized)
cx logs query "source logs | limit 10" --since now-15m -o agents
```

**Exercise:** Write a query that finds log lines containing the word "timeout" from the last 30 minutes. Show only the timestamp, severity, and message fields using a `select` clause.

<details>
<summary>Hint</summary>

```bash
cx logs query "source logs | filter $text ~ 'timeout' | select \$timestamp, \$severity, \$text | limit 50" \
  --since now-30m --until now
```

</details>

---

### 1.3 Query metrics

```bash
# Instant PromQL query
cx metrics query "up" --time now

# Range query - CPU usage over the last hour
cx metrics query 'rate(process_cpu_seconds_total[5m])' --since now-1h --until now --step 5m

# Search for available metric names
cx metrics search "cpu"
```

**Exercise:** Find all metrics that contain the word "error" in their name. Then pick one and run an instant query on it.

---

### 1.4 Check alerts

```bash
# List all alert definitions
cx alerts list

# View alerts in JSON
cx alerts list -o json

# Get details of a specific alert
cx alerts get <alert-id>
```

**Exercise:** How many alerts are currently defined? Are any of them currently triggered? Use `cx alerts list -o json | jq '.[] | select(.is_active == true)'` to filter.

---

### 1.5 Check data usage

```bash
# Today's usage
cx usage daily

# Last 7 days
cx usage daily --since now-7d --until now
```

**Exercise:** What is your team's average daily usage in units over the past week?

---

## Part 2: Using cx Through Claude Code Skills

Claude Code has skills that know how to use `cx`. Instead of remembering exact syntax, you describe your intent and Claude drives the CLI.

### 2.1 Start a telemetry investigation

Open a Claude Code session in this repo and type:

```
I want to investigate why our checkout service has elevated error rates over the last hour
```

Claude should load the `cx-telemetry-querying` skill and:
1. Ask clarifying questions (or make reasonable assumptions)
2. Run `cx metrics search` or `cx search-fields` to discover relevant signals
3. Run the appropriate `cx logs query` or `cx metrics query`
4. Summarize findings

**What to observe:** Notice how Claude picks between logs, metrics, and spans based on the question type. Watch the DataPrime or PromQL queries it constructs.

---

### 2.2 Cost and quota check

Ask Claude:

```
What's our current data ingestion trend and TCO policy status?
```

This should invoke the `cx-cost-optimization` skill, which runs:
- `cx usage daily`
- `cx tco list`

**Exercise:** After Claude runs this, ask a follow-up:

```
Which TCO policy has the highest impact on cost reduction?
```

Notice how Claude uses the previous context without re-running all the queries.

---

### 2.3 Alert investigation

Ask Claude:

```
Show me all active alerts and for each one, tell me what DataPrime query I could run to see the underlying data
```

Claude should use the `cx-alerts` skill and combine alert data with DataPrime knowledge.

---

## Part 3: Agent-to-Agent (A2A) with cx olly

`cx olly` is an AI-powered observability assistant built into Coralogix. Unlike direct CLI queries, `olly` understands natural language and can perform multi-step investigations internally - it reasons over your data and returns a synthesized answer.

This is the A2A pattern: **Claude Code (outer agent) drives `cx olly` (inner agent)**, which in turn queries Coralogix telemetry.

### 3.1 Understand the olly command

```bash
# See olly's subcommands
cx olly --help

# See all ask options
cx olly ask --help
```

Key flags:
| Flag | Purpose |
|------|---------|
| `--mode fast` | Quick answer, less analysis |
| `--mode focus` | Default - balanced depth |
| `--model` | Choose AI model (gpt-5.2, claude-sonnet-4-5, etc.) |
| `--chat-id` | Continue a previous conversation |
| `--timeout` | Max wait in seconds (default 900) |
| `-o agents` | Machine-readable output for agent consumption |

---

### 3.2 First olly query (human mode)

```bash
# Ask a natural language observability question
cx olly ask "What services had the highest error rate in the last hour?"
```

Note the output structure:
- **Chat ID** - Save this to continue the conversation
- **Status** - Should be `completed`
- **Response** - The AI's analysis
- **Artifacts** - Optional downloadable charts/tables

```bash
# Continue the conversation with follow-up
cx olly ask "Show me the top 5 error messages for the worst service" --chat-id <chat-id-from-above>
```

**Exercise:** Start a conversation asking about your system's health in the last 15 minutes. Continue the conversation with at least one follow-up question.

---

### 3.3 Olly in agents output mode

When Claude Code calls `cx olly`, it uses `-o agents` for compact, structured output:

```bash
# Agents-format output - what Claude Code actually receives
cx olly ask "What is the current p99 latency for the payment service?" -o agents
```

Observe how the output is more compact and machine-parseable compared to the default text output.

---

### 3.4 A2A exercise - Claude drives olly

This is the core A2A exercise. Open a Claude Code session and type:

```
Use cx olly to investigate whether there are any anomalies in our system in the last 30 minutes. 
Ask it at least two follow-up questions to drill down on anything interesting it finds.
Present a final summary of what you learned.
```

**What happens under the hood:**
1. Claude Code runs `cx olly ask "..."` with `-o agents`
2. Parses the structured JSON response
3. Extracts the chat ID
4. Runs `cx olly ask "..." --chat-id <id>` for follow-ups
5. Synthesizes a final answer from the conversation thread

**What to observe:**
- How Claude constructs each olly question based on previous responses
- How it handles the chat continuation pattern
- How it interprets AI-generated analysis from a remote agent

---

### 3.5 A2A with artifact handling

Olly can return artifacts (charts, tables, raw data). Ask Claude Code:

```
Use cx olly to get a breakdown of data ingestion by application over the last 24 hours. 
If olly returns any artifacts, read and summarize their content.
```

If olly returns an artifact, Claude will receive a file path to a spilled temp file (e.g., `/tmp/cx_results_artifact_<id>_<hash>.txt`) and can read it directly.

---

### 3.6 Combine olly with direct CLI queries

The most powerful pattern is having Claude validate olly's findings with direct CLI queries:

```
Use cx olly to identify which service is generating the most errors. 
Then use cx logs query to pull the raw error logs for that service and show me the top 3 distinct error messages.
```

This exercises the full A2A chain:
1. `cx olly ask` (AI analysis) - identifies the service
2. `cx logs query` (direct DataPrime) - retrieves raw evidence
3. Claude synthesizes both into a coherent answer

---

## Part 4: Reflection Exercises

### 4.1 When to use which approach?

Fill in the table based on your experience:

| Scenario | Best approach | Why |
|----------|--------------|-----|
| I need the exact log line that caused a crash | | |
| I want to know if anything looks unusual today | | |
| I need to build a PromQL dashboard query | | |
| I want to understand the business impact of an outage | | |
| I need to audit who changed an alert | | |

<details>
<summary>Suggested answers</summary>

| Scenario | Best approach |
|----------|--------------|
| Exact log line for a crash | `cx logs query` with DataPrime filter |
| Unusual activity today | `cx olly ask` - broad synthesis question |
| Build a PromQL query | Claude Code with `cx-telemetry-querying` skill |
| Business impact of outage | `cx olly ask` - AI reasoning across telemetry |
| Audit who changed an alert | `cx alerts get <id>` or `cx iam` |

</details>

---

### 4.2 Build a runbook

Using what you learned, write a 5-command runbook for the following incident scenario:

> **Scenario:** Users are reporting slow page loads on the checkout flow. You're on-call and need to triage in under 5 minutes.

Your runbook should include:
1. One `cx olly ask` for a quick AI-powered triage summary
2. One `cx metrics query` to check a relevant latency metric
3. One `cx logs query` to find error-level logs from the checkout service
4. One `cx alerts list` check to see if any alerts fired
5. One follow-up `cx olly ask --chat-id` question based on what you found

---

### 4.3 Skill discovery

Type this into a Claude Code session:

```
What cx commands are available for managing notifications and webhooks?
```

Claude should use `cx schema` or its built-in knowledge to answer without running random commands. Notice how the `cx schema` output serves as a self-describing API contract for the agent.

---

## Summary

| Skill learned | Key command |
|--------------|-------------|
| Log search | `cx logs query "<dataprime>"` |
| Metric query | `cx metrics query "<promql>"` |
| Alert management | `cx alerts list / get` |
| Usage monitoring | `cx usage daily` |
| AI-powered analysis | `cx olly ask "<question>"` |
| Multi-turn conversations | `cx olly ask ... --chat-id <id>` |
| Agent output format | `-o agents` on any command |
| Agent-to-agent flow | Claude Code + `cx olly` via `-o agents` |

**Key insight:** `cx` is both a human CLI and an agent API. The `-o agents` flag and `cx schema` make it machine-readable. `cx olly` adds a second AI layer - Claude Code acts as an orchestrator that delegates complex observability reasoning to Coralogix's built-in AI, then acts on the results.
