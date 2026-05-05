# Phase 5: Live-verify every query through the cx CLI

Every PromQL and DataPrime query in the draft dashboard must successfully run through `cx` before Phase 7 ships it. This is where invented metric names, typoed field paths, and malformed DataPrime pipelines get caught.

---

## 1. Resolve the dashboard time range

Parse `relativeTimeFrame` from the draft (default `"172800s"` = 48h) into a human token and call it `$RANGE`:

| `relativeTimeFrame` | `$RANGE` token |
|---|---|
| `3600s`   | `1h` |
| `21600s`  | `6h` |
| `86400s`  | `24h` |
| `172800s` | `48h` |
| `604800s` | `7d` |

Every verification query uses `$RANGE` so the CLI check matches what the dashboard will evaluate post-import. Don't substitute a hard-coded `[5m]` - that would test a different window than the dashboard runs.

---

## 2. Pick the storage tier for DataPrime verification (Frequent Search vs Archive)

`cx logs` and `cx spans` support a `--tier` flag:

- `--tier frequent` (default): hot storage, fast, recent data.
- `--tier archive`: cold/long-term storage, older data.

Use **Frequent Search** unless you have a reason to validate against Archive. Switch to **Archive** when:

- The dashboard is intended for long lookbacks (weekly/monthly trends, retrospectives).
- The widget's time range (via `$RANGE`) likely exceeds hot retention and Frequent Search returns empty for known-good queries.
- The user explicitly says “this dashboard should work on archived data.”

This tier choice affects only the **Phase 5 CLI verification commands** (execution-time). It does not modify the dashboard JSON.

---

## 3. Verify each PromQL query

For every widget whose definition contains a `promqlQuery`, substitute `${__range}` in the expression with `[$RANGE]` (e.g. `[48h]`). Leave any other fixed window (`[5m]`, `[1h]`) untouched - those were placed intentionally for sliding-rate panels.

**Instant-style widgets** (`gauge` / `pieChart` / `dataTable` with `promqlQueryType: PROM_QL_QUERY_TYPE_INSTANT`):

```bash
cx metrics query '<expression-with-[$RANGE]-substituted>' -o agents
```

**Time-series widgets** (`lineChart`):

```bash
cx metrics query-range '<expression>' --start now-$RANGE --end now --step <auto> -o agents
```

Pick `<step>` proportional to `$RANGE`: `1m` for 1–6h, `5m` for 24h, `1h` for 7d+. Match any window used by a `*_over_time` / `rate` / `increase` inside the expression if it's narrower than `$RANGE`.

A query **passes** when the CLI returns a 200 response and either has data or an empty-but-well-formed result. **Fails** include unknown metric names, parse errors, non-200 responses, or `cx` error output.

On failure: consult the `cx-metrics-query` skill for PromQL help, re-search for the real metric name with `cx metrics search`, re-list labels with `cx metrics get-labels`, and fix the query in the draft JSON. Budget ≤5 retry attempts per query.

---

## 4. Verify each DataPrime query

For every widget whose definition contains a `dataprimeQuery`, pick the CLI command from the widget's source prefix and **strip the leading `source logs` / `source spans`** before handing the pipeline to `cx`:

| Widget prefix | CLI | What to pass |
|---|---|---|
| `source logs \| …` | `cx logs` | everything after `source logs \|` (trim the leading `\|` and whitespace) |
| `source spans \| …` | `cx spans` | everything after `source spans \|` |

The dashboard runtime requires the `source …` prefix inside the widget JSON (see `query-syntax.md` §3), but `cx logs` and `cx spans` both inject the source themselves and will reject a pipeline that starts with `source …`. Strip it only for verification; restore nothing - the widget JSON keeps the prefix.

**Log-backed widgets:**

```bash
cx logs '<pipeline-without-leading-source-logs>' --start now-$RANGE --end now --limit 1 --tier <frequent|archive>
```

**Span-backed widgets:**

```bash
cx spans '<pipeline-without-leading-source-spans>' --start now-$RANGE --end now --limit 1 --tier <frequent|archive>
```

A query **passes** when the CLI returns without a parse error. Empty results are acceptable. On failure: consult the `cx-dataprime` skill (`cx dataprime show <command>` for inline help), re-discover fields with `cx search-fields`, fix, retry. Budget ≤5 retry attempts per query.

---

## 5. Restore `${__range}` before Phase 6

Once every PromQL and DataPrime query passes, restore `${__range}` (and any other variables) in the emitted JSON. The verification step uses the concrete `$RANGE`; the final JSON keeps the injected variables intact.

If any query can't be made to pass within the retry budget, surface it to the user with the CLI error verbatim - don't silently ship a broken widget.
