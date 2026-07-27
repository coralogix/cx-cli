# Onboarding reference MD — authoring template

> **What this is.** The standard shape for a per-product onboarding reference file loaded by the
> `cx-onboarding` orchestrator skill (`skills/cx-onboarding/references/onboarding-<product>.md`).
> Copy this file, rename it, and fill every section. The orchestrator handles *routing and
> verification*; your reference handles *the prerequisites and steps for one signal*.
>
> **Who owns it.** The PM for that product/signal. Author v1 from docs + Slack threads + support
> cases; refine with the field. Don't block the orchestrator on a perfect reference — a good v1
> that a PM iterates beats a blank slot.
>
> **Golden rules:**
> 1. **Encode prerequisites in order, and be explicit about format.** The #1 cause of "no data" is a
>    wrong format or a missing param (e.g. sending plain text where OTLP protobuf is required). State
>    it before the happy path, not in a footnote.
> 2. **Every reference ends by verifying data landed** with a read-only `cx` query. "Config applied"
>    is not "done".
> 3. **Layer-1 (no-AI) must work end to end.** AI assists are optional add-ons, never the only path.
> 4. **Keep exact per-region endpoints out of the file.** Read the region from the `cx` profile and
>    deep-link the endpoints doc — don't hardcode a table that goes stale.
> 5. **Instructions live here; docs are the reference.** Link to the canonical doc page for depth
>    instead of duplicating a wizard that will diverge (coordinate with the docs team on deep-links).

Delete this callout block in real reference files. Keep the section structure below.

---

# Onboarding: <Product / signal name>

One or two sentences: what this signal is and what "onboarded" means for it (what the user should
see in the product when it works).

## When to use this reference

The orchestrator loads this file when the user wants to `<intent phrases>`. Note anything that
should route elsewhere instead (e.g. "already flowing → cx-telemetry-querying").

## Prerequisites (in order)

Numbered, blocking, format-explicit. For each: what it is, how to check it, how to fix it.

1. **<prereq>** — e.g. collector running / SDK installed / endpoint reachable.
   ```bash
   <check command>
   ```
2. **Required format & params.** State the wire format (e.g. **OTLP protobuf over gRPC**) and the
   required parameters (endpoint, `Authorization: Bearer <send-your-data-api-key>`,
   `cx.application.name`, `cx.subsystem.name`, any signal-specific params). Call out anything that
   fails silently if wrong.

## Minimal config (happy path)

The smallest working setup. Prefer a copyable snippet with placeholders. Show the resource
attributes / headers that tag the data (`cx.application.name`, `cx.subsystem.name`).

```yaml
# minimal example — placeholders in <angle brackets>
```

## Verify (close the loop)

The exact read-only `cx` query that proves data arrived, plus what a healthy result looks like.

```bash
cx <logs|spans|metrics> "<query for this app/service>" --start now-15m --limit 5
```

Expected: `<what the user should see>`. If empty after ~5–10 min, see Common failures.

## Common failures → fixes

| Symptom | Likely cause | Fix |
|---|---|---|
| No data after 10 min | Wrong region/endpoint, or not OTLP protobuf | Match endpoint to profile region; confirm exporter format |
| Data lands under the wrong name | app/subsystem attributes unset or misspelled | Set `cx.application.name` / `cx.subsystem.name` |
| Rejected / dropped payloads | Payload > OTLP limit (hard 10 MB, ~2 MB recommended) | Batch / reduce payload size |
| <signal-specific> | <cause> | <fix> |

## Tier & cost

- **Tier interaction (Medium First):** how this signal behaves across High / Medium / Low / Block
  (e.g. features unavailable in Low). Don't guide a user toward a destination their tier blocks.
- **Customer cost:** ingress/egress implications and mitigations (batching, compression, PrivateLink).
- **Coralogix COGS:** note if any step absorbs AI tokens at Coralogix's cost (must be explicit).

## AI layers for this signal

- **Layer 1 (no-AI):** the deterministic steps above — always works.
- **Layer 2 (minimal free AI):** optional low-token assist, if any (say what, and which cheap model).
- **Layer 3 (full Olly, paid):** optional deep/autonomous assist, if any (credit-gated).

## Docs deep-links

- <canonical doc page 1>
- <canonical doc page 2>

## Sources / evidence

Where the steps came from (docs, support cases, first-hand knowledge). Cite sources so the reference
stays grounded and reviewable.

---

## Frontmatter for skill-local reference files

Reference files do **not** need YAML frontmatter (only `SKILL.md` does). Keep the filename
`lowercase-kebab-case.md`, prefixed `onboarding-` (e.g. `onboarding-apm-spans.md`), and make sure the
orchestrator's **Loading references** table links to it.
