# cx CLI reference

The `cx` CLI queries Coralogix directly: spans via DataPrime, plus official docs and AI Center config.

## Install

```bash
cx --version   # verify it's available
```

If not installed: `cx docs search "cli"` from another machine, or https://coralogix.com/docs/.

## Discovery

```bash
cx --help          # list all commands
cx spans --help    # args/options for a specific command
```

## Credentials

`cx profiles list` — check for a configured profile first; profiles live in `~/.cx`/`~/.config/cx`.

Set environment variables or use a configured profile:

```bash
export CX_API_KEY=cxup_...   # personal QUERY key — not a Send-Your-Data key
export CX_REGION=eu2
# or
cx spans "..." --profile <profile-name>
```

A Send-Your-Data key (used for ingest in the collector/OTLP headers) **cannot query** — every query fails with a "required scope" error. Keys are managed in Coralogix under Data Flow → API Keys.

## Querying GenAI spans

`cx spans` takes a DataPrime query; `source spans` is prepended automatically.

```bash
# Recent GenAI spans (AI Center reads the archive tier — query the same)
cx spans "filter tags['gen_ai.provider.name']:string != null
  | select $m.traceID, name,
           tags['gen_ai.request.model']:string,
           tags['gen_ai.usage.input_tokens']:string,
           tags['gen_ai.usage.output_tokens']:string
  | limit 10" --tier archive --start now-1h

# All spans of one trace
cx spans "filter $m.traceID == '<trace-id>' | limit 100" --tier archive --start now-1h

# Token usage per model
cx spans "filter tags['gen_ai.provider.name']:string != null
  | groupby tags['gen_ai.request.model']:string as model
      aggregate sum(tags['gen_ai.usage.input_tokens']:number) as input_tokens,
                sum(tags['gen_ai.usage.output_tokens']:number) as output_tokens" \
  --tier archive --start now-24h
```

## AI Center commands

```bash
# Is the app registered in the AI Center catalog? Prints a "View in Coralogix" URL
# plus a table of ID | Application | Subsystem | Guarded
cx ai-center applications list
cx ai-center applications get <id>
```

`cx ai-center applications list` prints `View in Coralogix: <url>` — copy it into the report.

```bash
# Cost overrides for models AI Center doesn't price automatically
# (unknown or self-hosted models otherwise show $0 cost)
cx ai-center model-pricing get
cx ai-center model-pricing set --from-file pricing.json --yes
```

## Tips

- **Pick the tier by purpose.** Use the **frequent** tier for fast arrival checks (spans land within seconds) — if a `tags[...]` filter returns nothing there, filter by application/subsystem (`$l.applicationName` / `$l.subsystemName`) instead. Use the **archive** tier (`--tier archive`) for every gate — AI Center reads only the archive, and archived spans can take minutes to ~30 minutes to become queryable after ingest.
- Span attributes live under `tags[...]` and are typed with `:string` / `:number` casts; metadata (traceID, spanID, duration) lives under `$m.`.
- Time ranges: `--start` / `--end` accept ISO 8601 or relative (`now-1h`). For reproducible comparisons use absolute windows, never `now-Xh`.
- `--limit` caps results (default 200); aggregate with `groupby ... aggregate ...` instead of pulling raw spans when you need counts/sums.
- Use `cx search-fields` to discover span/log field names by description or value content.
- `cx docs search` / `cx docs fetch` give you official product documentation as markdown.
