# Onboarding: AI Center / LLM telemetry  *(STUB — not yet authored)*

> **Status:** not yet authored. AI Center onboards over OpenTelemetry like the other signals; this is a
> thin routable stub so the orchestrator can hand off cleanly. Complete it from
> `contributing/onboarding-reference-md-template.md`.

Send LLM / AI-agent telemetry (prompts, completions, token usage, evals, guardrails) into Coralogix
AI Center. "Onboarded" means: AI spans/metrics appear in AI Center with model and token data.

## When to use this reference

"Monitor my LLM app", "send AI/agent telemetry", "set up AI Center", "track token usage / model
calls / guardrails".

## Prerequisites (in order)

1. **A collector or OTLP destination** (as for spans/metrics) — AI Center consumes OpenTelemetry.
2. **Required format & params — OTLP protobuf over gRPC**, `ingress.<coralogix-domain>:443`,
   `Authorization: Bearer <send-your-data-api-key>`, with `cx.application.name` / `cx.subsystem.name`.
3. **GenAI semantic conventions** on spans (model, tokens, prompt/response) so AI Center can parse them.
   *TODO (AI Center PM):* confirm the exact attributes and any Coralogix-specific ones.

## Minimal config (happy path)

*TODO (AI Center PM):* the OTel GenAI instrumentation snippet (e.g. an OpenAI/LLM auto-instrumentation
library) exporting to the endpoint/collector, per the
[AI Center OTel integration doc](https://coralogix.com/docs/user-guides/ai/otel-integration/).

## Verify (close the loop)

```bash
cx spans "filter \$l.serviceName == '<ai-service>'" --start now-15m --limit 5
```
*TODO:* the specific attribute/metric that confirms AI Center parsed it (model / token count).

## Common failures → fixes / Tier & cost / AI layers / Docs deep-links

*TODO (AI Center PM)* — follow the template. **Note the meta-point:** AI Center is *per-team* billed
(evals/guardrails), distinct from *per-user* Olly. Docs:
[AI Center OpenTelemetry integration](https://coralogix.com/docs/user-guides/ai/otel-integration/).

## Sources / evidence

Scaffold (2026-07). AI Center OpenTelemetry integration doc (verified). Onboarding order = last (after RUM).
