# Health reference MD — authoring template

> **What this is.** The standard shape for a per-experience/extension health reference loaded by the
> `cx-system-health` orchestrator (`skills/cx-system-health/references/health-<surface>.md`). Copy,
> rename, fill every section. The orchestrator handles the verdict model + routing; your reference
> defines *the conditions for one surface and how to verify them*.
>
> **Sibling template:** for *getting data in*, use `onboarding-reference-md-template.md` instead. This
> one is for *checking data already there is healthy/complete*.
>
> **Golden rules:**
> 1. **Every condition is checkable read-only** with a `cx` query — no mutation to assess health.
> 2. **Every failing condition names the reason AND the route to fix it** (usually back to
>    `cx-onboarding`). A verdict with no remediation is not actionable.
> 3. **Distinguish a data verdict from a tier verdict** — "healthy data but the experience is hidden by
>    tier" is a different answer than "no data".
> 4. **Write conditions, not internal APIs.** The backing verdict source may change — keep the
>    reference to "here's the condition and how to verify it" so it survives either design.

Delete this callout in real reference files. Keep the structure below.

---

# Health: <Experience / extension name>

One or two sentences: what this surface needs telemetry to look like to deliver value.

## When to use this reference

Intent phrases that route here. Note anything that should go elsewhere (e.g. "never set up →
cx-onboarding"; "investigate a specific error → cx-telemetry-querying").

## Conditions & checks

A table — one row per condition. Kinds: Presence / Continuity / Completeness / Quality.

| # | Condition | Kind | Check (read-only `cx` query) | Fail → |
|---|---|---|---|---|
| 1 | <the signal arrives now> | Presence | `cx <...>` | **missing** → route |
| 2 | <required attribute present> | Completeness | `cx <...> -o json` / `cx search-fields` | **degraded** → fix |
| 3 | <quality bar> | Quality | `<check>` | **degraded** → fix |

## Verdict → remediation

For each verdict, the concrete route: which `cx-onboarding` reference (or other skill) fixes it, and
the smallest change that flips it back to healthy.

## Surface-specific gotchas

The traps unique to this surface (e.g. accidental 100% sampling; naming fragmentation; blank service
names). These are where "looks healthy but isn't useful" hides.

## Tier note

How tier affects whether the *experience* is visible even with healthy data. Separate tier verdicts
from data verdicts.

## AI layers

- **Layer 1 (no-AI):** the condition table.
- **Layer 2 (minimal free):** optional — rank/phrase verdicts (cheap model; absorbed cost in COGS).
- **Layer 3 (Olly, paid):** optional — diagnose + propose fix (credit-gated).

## Docs deep-links

- <canonical doc page(s)>

## Sources / evidence

Where the conditions came from (docs, support cases, first-hand knowledge). Cite sources.
