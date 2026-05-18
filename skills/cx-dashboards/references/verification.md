# Phase 5: Live-verify every query through the cx CLI

Every PromQL and DataPrime query in the draft dashboard must successfully run through `cx` before Phase 7 ships it. This is where invented metric names, typoed field paths, and malformed DataPrime pipelines get caught.

---

## 1. Resolve the dashboard time range (PromQL only)

Parse `relativeTimeFrame` from the draft (default `"172800s"` = 48h) into a human token and call it `$RANGE`:

| `relativeTimeFrame` | `$RANGE` token |
|---|---|
| `3600s`   | `1h` |
| `21600s`  | `6h` |
| `86400s`  | `24h` |
| `172800s` | `48h` |
| `604800s` | `7d` |

`$RANGE` is used **only** for PromQL verification (§2): range vectors are window-sensitive, so the CLI check has to match the window the dashboard will evaluate. DataPrime verification (§3) uses a fixed short window instead — see that section.

---

## 2. Verify each PromQL query

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

## 3. Verify each DataPrime query

For every widget whose definition contains a `dataprimeQuery`, pick the CLI command from the widget's source prefix and **strip the leading `source logs` / `source spans`** before handing the pipeline to `cx`:

| Widget prefix | CLI | What to pass |
|---|---|---|
| `source logs \| …` | `cx logs` | everything after `source logs \|` (trim the leading `\|` and whitespace) |
| `source spans \| …` | `cx spans` | everything after `source spans \|` |

The dashboard runtime requires the `source …` prefix inside the widget JSON (see `query-syntax.md` §3). `cx logs` and `cx spans` inject the source themselves; if you leave a leading `source …` in the pipeline they silently run against the command's own source, which masks pillar mismatches. Strip it for verification; restore nothing - the widget JSON keeps the prefix.

Verify against a **fixed short window** (`now-15m` → `now`), not the dashboard's `$RANGE`. The goal here is syntax / field / pipeline validation — proving the query parses and references real fields. The dashboard runs against `${__range}` itself at render time; we don't need to re-prove data presence on the dashboard's window during the build. A short window is faster, cheaper, and a clean fail signal (a query that fails on `now-15m` is broken regardless of range).

Choose the tier to verify:

- `--tier frequent` (default): hot storage, fast, recent data.
- `--tier archive`: cold/long-term storage, older data.

Use **Frequent Search** unless you have a reason to validate against Archive. Switch to **Archive** when:

- The dashboard is intended for long lookbacks (weekly/monthly trends, retrospectives).
- Frequent Search returns empty for known-good queries because the time range is beyond hot retention.
- The user explicitly says “this dashboard should work on archived data.”

**Log-backed widgets:**

```bash
cx logs '<pipeline-without-leading-source-logs>' --start now-15m --end now --limit 1 --tier <frequent|archive>
```

**Span-backed widgets:**

```bash
cx spans '<pipeline-without-leading-source-spans>' --start now-15m --end now --limit 1 --tier <frequent|archive>
```

Check both the exit code and the output — some errors surface only in the output. A query **passes** when `cx` exits 0 and the output is rows or `[]` with no error or warning lines (an empty result on a low-volume signal is fine). It **hard-fails** on a non-zero exit, an `error from profile '...': API request failed` line, or a `Compilation errors:` block — the query is broken. A `keypath does not exist` warning is a **soft fail**: the query parsed but no record in the window had the referenced field. Verify with `cx search-fields "<hint>" --dataset logs|spans`; if the field is real the query is fine (widen the window or accept the empty result), if it isn't, fix the field name. On failure: consult the `cx-dataprime` skill (`cx dataprime show <command>` for inline help), re-discover fields with `cx search-fields`, fix, retry. Budget ≤5 retry attempts per query.

---

## 4. Restore `${__range}` before Phase 6

Once every PromQL and DataPrime query passes, restore `${__range}` (and any other variables) in the emitted JSON. PromQL verification swapped `${__range}` for the concrete `[$RANGE]`; the final JSON keeps the injected variable intact. (DataPrime queries don't carry `${__range}`, so nothing to restore there beyond keeping the `source logs` / `source spans` prefix in the widget JSON.)

If any query can't be made to pass within the retry budget, surface it to the user with the CLI error verbatim - don't silently ship a broken widget.
