# Copilot CLI Reference

Copilot has two distinct data shapes — load the references for the one in use:

| Data Shape | When to Use | References |
|---|---|---|
| Direct OTel spans | User ran the Copilot CLI with OTel instrumentation | `dataprime-reference.md` + `spans-querying.md` |
| GitHub Copilot Collector metrics | Question is about GitHub org usage, billing, IDEs, languages, features, or PRs | `promql-guidelines.md` + `metrics-querying.md` |

> If unclear which shape applies, ask the user whether their Copilot data comes from direct CLI spans or from the GitHub Copilot Collector.

---

## Direct Copilot CLI OTel Spans

### Span Data Model

Base source filter:

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
```

| Tag / Operation | Description |
|---|---|
| `tags['gen_ai.operation.name'] == 'invoke_agent'` | Root user-message span — use for cost, user, model, token, and session totals |
| `tags['gen_ai.operation.name'] == 'chat'` | LLM API call span — use for round-trip duration |
| `tags['gen_ai.operation.name'] == 'execute_tool'` | Tool call span — use for tool counts |
| `tags['enduser.pseudo.id']` | Pseudonymous user identifier (not an email) |
| `tags['gen_ai.conversation.id']` | Session / conversation identifier |
| `tags['gen_ai.request.model']` | Model name |
| `tags['github.copilot.cost']` | Cost for the `invoke_agent` span |
| `tags['gen_ai.usage.input_tokens']` | Input tokens (on `invoke_agent`) |
| `tags['gen_ai.usage.output_tokens']` | Output tokens (on `invoke_agent`) |
| `tags['gen_ai.usage.cache_read.input_tokens']` | Cache-read input tokens (on `invoke_agent`) |
| `tags['gen_ai.tool.name']` | Tool name (on `execute_tool`) |

> **Important:** Do not filter `chat` or `execute_tool` spans by user unless sample data confirms the user tag is propagated there. Prefer `invoke_agent` spans for user-level aggregations.

### Direct OTel Span Queries

#### Total Cost

```bash
cx spans 'filter $l.serviceName == "github-copilot" || tags["otel.scope.name"] == "github.copilot"
| filter tags["gen_ai.operation.name"] == "invoke_agent"
| create cost from tags["github.copilot.cost"]:number
| aggregate sum(cost) as totalCost
| choose totalCost' --start now-7d -o agents
```

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'invoke_agent'
| create cost from tags['github.copilot.cost']:number
| aggregate sum(cost) as totalCost
| choose totalCost
```

#### Token Totals

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'invoke_agent'
| create input from tags['gen_ai.usage.input_tokens']:number
| create output from tags['gen_ai.usage.output_tokens']:number
| create cacheRead from tags['gen_ai.usage.cache_read.input_tokens']:number
| aggregate sum(input) as inputTokens, sum(output) as outputTokens, sum(cacheRead) as cacheReadTokens
| create totalTokens from inputTokens + outputTokens
| choose totalTokens, inputTokens, outputTokens, cacheReadTokens
```

#### Unique Sessions

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'invoke_agent'
| create conversation_id from tags['gen_ai.conversation.id']
| filter conversation_id != null
| aggregate approx_count_distinct(conversation_id) as totalSessions
| choose totalSessions
```

#### Top Users by Cost

```bash
cx spans 'filter $l.serviceName == "github-copilot" || tags["otel.scope.name"] == "github.copilot"
| filter tags["gen_ai.operation.name"] == "invoke_agent"
| create user from tags["enduser.pseudo.id"]
| filter user != null
| create cost from tags["github.copilot.cost"]:number
| groupby user aggregate sum(cost) as totalCost
| sort by totalCost desc
| limit 10
| choose user as userEmail, totalCost' --start now-7d -o agents
```

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'invoke_agent'
| create user from tags['enduser.pseudo.id']
| filter user != null
| create cost from tags['github.copilot.cost']:number
| groupby user aggregate sum(cost) as totalCost
| sort by totalCost desc
| limit <N>
| choose user as userEmail, totalCost
```

#### Cost and Tokens by Model

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'invoke_agent'
| create model from tags['gen_ai.request.model']
| filter model != null
| create cost from tags['github.copilot.cost']:number
| groupby model aggregate sum(cost) as totalCost
| sort by totalCost desc
| choose model, totalCost
```

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'invoke_agent'
| create model from tags['gen_ai.request.model']
| filter model != null
| create input from tags['gen_ai.usage.input_tokens']:number
| create output from tags['gen_ai.usage.output_tokens']:number
| groupby model aggregate sum(input) as totalInput, sum(output) as totalOutput
| create totalTokens from totalInput + totalOutput
| sort by totalTokens desc
| choose model, totalTokens
```

#### LLM Round-Trip Duration (`chat` spans)

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'chat'
| create dur_ms from $m.duration:number / 1000
| aggregate sum(dur_ms) as totalDurationMs, avg(dur_ms) as avgDurationMs, count() as chatCount
| choose totalDurationMs, avgDurationMs, chatCount
```

#### Top Tools by Call Count

```text
source spans
| filter $l.serviceName == 'github-copilot' || tags['otel.scope.name'] == 'github.copilot'
| filter tags['gen_ai.operation.name'] == 'execute_tool'
| create tool_name from tags['gen_ai.tool.name']
| filter tool_name != null
| countby tool_name into tool_count desc
| limit <N>
```

#### User Drilldown

Append to any `invoke_agent` query above:

```text
| filter tags['enduser.pseudo.id'] == '<pseudo_user_id>'
```

> Users are identified by pseudonymous ID, not email. Make this clear when presenting results.

---

## GitHub Copilot Collector Metrics

Use these when the question is about GitHub organization usage, billing, IDEs, languages, features, PRs, or user-level GitHub Copilot adoption. These metrics are **not** the same as direct CLI spans.

### Metrics Data Model

Important labels: `organization`, `user_login`, `user_email`, `user_name`, `sku`, `unit_type`, `ide`, `feature`, `language`, `model`, `phase`.

> Do not add an `organization` matcher by default. Add `{organization="<org>"}` only when the user explicitly wants a single organization.

| Metric Family | Description |
|---|---|
| `github_copilot_org_daily_active_users` | Daily active users (org-wide) |
| `github_copilot_org_weekly_active_users` | Weekly active users |
| `github_copilot_org_monthly_active_users` | Monthly active users |
| `github_copilot_org_cli_session_count` | CLI sessions (org) |
| `github_copilot_org_cli_request_count` | CLI requests (org) |
| `github_copilot_org_cli_prompt_tokens_sum` | CLI prompt tokens (org) |
| `github_copilot_org_cli_output_tokens_sum` | CLI output tokens (org) |
| `github_copilot_billing_net_amount` | Net billing amount |
| `github_copilot_billing_gross_amount` | Gross billing amount |
| `github_copilot_billing_discount_amount` | Discount amount |
| `github_copilot_billing_net_quantity` | Net billed quantity |
| `github_copilot_org_user_initiated_interaction_count_by_model_feature` | Interactions by model and feature |
| `github_copilot_org_user_initiated_interaction_count_by_ide` | Interactions by IDE |
| `github_copilot_org_user_initiated_interaction_count_by_feature` | Interactions by feature |
| `github_copilot_org_loc_added_sum_by_language_feature` | Lines added by language and feature |
| `github_copilot_org_loc_deleted_sum_by_language_feature` | Lines deleted |
| `github_copilot_org_loc_suggested_to_add_sum` | Lines suggested to add |
| `github_copilot_org_pull_requests_*` | PR metrics (created, reviewed, merged, Copilot-created, Copilot-reviewed, suggestions, applied suggestions, median minutes to merge) |
| `github_copilot_user_cli_session_count` | CLI sessions (per user) |
| `github_copilot_user_cli_prompt_tokens_sum` | CLI prompt tokens (per user) |
| `github_copilot_user_cli_output_tokens_sum` | CLI output tokens (per user) |
| `github_copilot_user_loc_added_sum` | Lines added (per user) |
| `github_copilot_user_code_acceptance_count` | Code acceptances (per user) |
| `github_copilot_user_user_initiated_interaction_count` | Interactions (per user) |
| `github_copilot_user_user_initiated_interaction_count_by_model_feature` | Interactions by model and feature (per user) |

### Collector PromQL Queries

> These are daily gauges/sums. Use `sum_over_time` rather than `increase`.

```bash
# Total CLI sessions for the last week
cx metrics query-range 'sum(sum_over_time(github_copilot_org_cli_session_count[1w]))' \
  --start now-7d
```

```promql
sum(sum_over_time(github_copilot_org_cli_session_count[<range>]))
```

```promql
sum(sum_over_time(github_copilot_org_cli_prompt_tokens_sum[<range>]))
+
sum(sum_over_time(github_copilot_org_cli_output_tokens_sum[<range>]))
```

```promql
sum(sum_over_time(github_copilot_billing_net_amount[<range>]))
```

```promql
sum by (sku) (sum_over_time(github_copilot_billing_net_amount[<range>]))
```

```promql
topk(<N>, sum by (model) (sum_over_time(github_copilot_org_user_initiated_interaction_count_by_model_feature[<range>])))
```

```promql
topk(<N>, sum by (ide) (sum_over_time(github_copilot_org_user_initiated_interaction_count_by_ide[<range>])))
```

```promql
topk(<N>, sum by (language) (sum_over_time(github_copilot_org_loc_added_sum_by_language_feature[<range>])))
```

```promql
sum(sum_over_time(github_copilot_org_pull_requests_copilot_applied_suggestions[<range>]))
/
sum(sum_over_time(github_copilot_org_pull_requests_copilot_suggestions[<range>]))
* 100
```

> **User identification:** user metrics may identify users by email or by `user_login` + `user_name`. Prefer `user_email` where present; otherwise join login and name as `<login> (<name>)`.
