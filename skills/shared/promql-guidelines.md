# PromQL Guidelines

## Core Principles

1. **Pick the right query type**
   - **Instant queries** (`cx metrics query`) evaluate an expression at a single timestamp (now, or a given `--time`). Use when the question requires **one number or one vector** *as of* a moment - essentially any query that does not require results over different timeframes.
   - **Range queries** (`cx metrics query-range`) evaluate the expression **repeatedly** across `[--start, --end]` at a given `--step`. Use for **time series** over a period (e.g., daily active users per day).
     - Note: Range queries **evaluate the expression repeatedly** at each step. If `--step=1d`, `--start=now-1d`, `--end=now`, and the query is `max_over_time(metric[1d])`, the query evaluates at `now-1d` and `now` - two evaluations covering two days of data.
   - Prefer instant queries over range queries for most questions, except when comparing different timeframes.

2. **Understand PromQL value types**
   - **Instant vector** - set of series with 1 sample each at eval time
   - **Range vector** - series with many samples over a window `[t-range, t]`
   - **Scalar** - single number
   - **String** - rare
   - Functions like `*_over_time()` **require a range vector**. Aggregations like `sum/max/min/avg ... by(...)` **consume instant vectors**.
   - **Important**: When using `*_over_time()` functions with range queries, be aware that the query also evaluates at the `--start` time and includes the window specified in the function.
   - **Example**: If `max_over_time(metric[1d])` is used with `--start=now-1d`, `--end=now`, `--step=1d`, the query evaluates at `now-1d` and `now` - the result is the max over `[now-2d, now]`. This is a common mistake. If a user asks "What is the max of x between 2025-01-01 and 2025-01-07?" and `max_over_time(x[7d])` is used with `--start=2025-01-01`, `--end=2025-01-07`, `--step=1d`, the evaluation at `2025-01-01` includes `[2024-12-25, 2025-01-01]` - which is wrong. Use an instant query with `--time` to avoid this.

3. **Separation of concerns**
   - Use `*_over_time()` for **temporal reductions** across a window (e.g., `max_over_time`, `avg_over_time`, `quantile_over_time`).
   - Use `sum/max/min/avg by (...)` for **label-set aggregation** across series at the eval point.
   - Chain them as needed (temporal reduction first, then label aggregation, or vice versa).

4. **Counters vs. gauges**
   - **Counters** (monotonic, suffixed `_total`) → use `rate()`/`irate()` or `increase()` over a window.
   - **Gauges** (current value) → use `avg_over_time`, `max_over_time`, etc., or plain `avg(...)` depending on intent.

5. **Suffix conventions**
   - Canonical: `_total` (counter), `_bucket/_sum/_count` (histogram), `_sum/_count` (summary), `_created`.
   - Non-standard: `_avg`, `_mean`, etc. Prefer computing averages via PromQL unless the exporter dictates otherwise.

---

## CLI Usage

### Instant Query

```bash
cx metrics query '<expr>'
cx metrics query '<expr>' --time 2024-01-01T12:00:00Z
cx metrics query '<expr>' --output json
```

**Example: absolute max over last 24h (single result)**

```bash
cx metrics query 'max by () (max_over_time(http_requests_in_flight[24h]))'
```

### Range Query

```bash
cx metrics query-range '<expr>' --start now-7d --end now --step 1d
```

**Example: absolute max per day over the last 7 days**

```bash
cx metrics query-range 'max by () (max_over_time(metric[1d]))' \
  --start now-7d --end now --step 1d
```

**IMPORTANT**: Align `--step` with any window used in temporal reduction functions. If using `max_over_time(metric[1d])`, set `--step 1d`.

---

## PromQL Fundamentals

### Label Matching & Aggregation

- Matchers: `{label="v"}`, `{label!="v"}`, `{label=~"re.*"}`, `{label!~"re"}`
- Aggregate **by** labels to keep them; use **without** to drop them.

```promql
sum by (job) (rate(http_requests_total[5m]))
sum without (instance) (up)
```

### Temporal Reductions (range → instant)

```promql
max_over_time(cpu_usage[1h])
avg_over_time(node_memory_Active_bytes[30m])
quantile_over_time(0.99, queue_length[1h])
```

### Counters: Rates, Increases, Windows

Per-instance RPS:
```promql
rate(http_requests_total[5m])
```

Total RPS across fleet:
```promql
sum by () (rate(http_requests_total[5m]))
```

Events in last day (per user, then count actives):
```promql
count( sum by (user_id) (increase(api_call_count[24h])) > 0 )
```

### Histograms & Summaries

p95 from a histogram:
```promql
histogram_quantile(
  0.95,
  sum by (le, route) (rate(http_request_duration_seconds_bucket[5m]))
)
```

Average from summary parts:
```promql
sum(rate(req_duration_seconds_sum[5m]))
/
sum(rate(req_duration_seconds_count[5m]))
```

### Max over a Period

Correct - temporal reduction, then aggregation:
```promql
max by () (max_over_time(metric[4d]))
```

Per-label max:
```promql
max by (label) (max_over_time(metric[4d]))
```

Incorrect - `max()` cannot take a range vector:
```promql
max(metric[4d])   ← error
```

### Top-k / Ranking

```promql
topk(5, sum by (instance) (rate(http_requests_total[5m])))
```

---

## Common Tasks (ready to adapt)

1. **Absolute peak per instance over 7d, then pick the winner**

```promql
topk(1, max by (instance) (max_over_time(my_metric[7d])))
```

Run as instant query (no `--time` needed - defaults to now).

2. **Global CPU usage % (avg across cores & hosts)**

```promql
avg by () (
  rate(process_cpu_seconds_total[5m])
) * 100
```

3. **Error rate (%) per route**

```promql
100 * sum by (route) (rate(http_requests_total{code=~"5.."}[5m]))
    / sum by (route) (rate(http_requests_total[5m]))
```

4. **Daily active users over a week (time series)**

Expression:
```promql
count(count by (user_id) (increase(api_call_count[1d]) > 0))
```

Run as range query with `--step 1d --start now-6d --end now`. (Starting from 6 days ago because `increase` looks back one full day from each evaluation point.)

---

## Performance & Safety Guidelines

- Prefer **short windows** for `rate()` (e.g., 1–5m) unless data is bursty or sparse.
- Avoid unbounded fan-out (e.g., joining massive label sets).
- Keep **cardinality** under control; aggregate early (`sum by (...)`) when only totals are needed.
- Use `clamp_max`/`clamp_min` to tame outliers when needed.
- For histograms, **always** aggregate buckets (`sum by (le, ...)`) before `histogram_quantile`.
- Be mindful of counter resets; `rate()`/`increase()` handle resets automatically.

---

## Frequent Gotchas (and fixes)

- **"Why am I getting a time series when I only want one number?"**
  Use `cx metrics query` (instant) instead of `cx metrics query-range`.

- **"`max(metric[...])` errors."**
  `max()` can't take a range vector. Use `max_over_time(metric[...])`, then aggregate with `max by () (...)`.

- **"`_over_time(metric[...]) by (label)` errors."**
  `_over_time` aggregations cannot include a `by` clause. Use `max_over_time(metric[...])`, then `max by (label) (...)`.

- **"Avg looks wrong for counters."**
  Counters need `rate()`/`increase()`, not `avg_over_time`.

- **"p95 from a summary?"**
  Summaries expose quantiles directly via the `quantile` label. For histograms, use `histogram_quantile` on bucket rates.

- **"Results show empty label values."**
  Add `{label!=""}` to the selector to filter out empty label values. Example: `max by (deployment) (rate(cpu_usage{deployment!=""}[5m]))`.

---

## Mini Cheat-Sheet

| Goal | PromQL |
|---|---|
| Rate of a counter | `rate(x_total[5m])` |
| Increase last 24h | `increase(x_total[24h])` |
| Avg of a gauge over 1h | `avg_over_time(x[1h])` |
| Max over 4d (absolute) | `max by () (max_over_time(x[4d]))` |
| Top 5 by RPS | `topk(5, sum by (instance) (rate(x_total[5m])))` |
| p95 latency (histogram) | `histogram_quantile(0.95, sum by (le) (rate(x_bucket[5m])))` |
| Filter labels | `{env="prod", job=~"api\|web"}` |
| Drop a label in agg | `sum without (instance) (x)` |
| Time travel | `expr @ <unix_ts>` or `expr offset 1h` |
