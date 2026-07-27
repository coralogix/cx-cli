# Onboarding: Logs  *(STUB — for the Logs PM to complete)*

> **Status:** scaffold. The orchestrator routes here, but a Logs PM should complete it from the
> template (`contributing/onboarding-reference-md-template.md`). The prerequisite below is the
> load-bearing lesson and should survive into v1.

Ship logs into Coralogix. "Onboarded" means: logs are queryable in Explore with the expected
application/subsystem names and severity.

## When to use this reference

"Ship logs to Coralogix", "my logs aren't arriving", "set up log collection", "send application logs".

## Prerequisites (in order)

1. **A destination.** The collector (`onboarding-fleet.md`) in Kubernetes, or direct OTLP export.
2. **Required format & params — this is the classic trap.** The OTLP logs endpoint expects
   **OpenTelemetry protobuf over gRPC**, not raw text. **Sending logs as plain text fails** —
   in a real onboarding this blocked ingestion until the sender was switched to OTLP **protobuf**.
   Confirm the log exporter emits OTLP protobuf, and set `cx.application.name` / `cx.subsystem.name`.
   *(Known failure mode: logs sent as plain text are rejected/silently dropped until the exporter is
   switched to OTLP protobuf.)*
3. **App / subsystem naming** decided up front.

## Minimal config (happy path)

*TODO (Logs PM):* smallest working config for (a) collector-collected container logs and (b) direct
OTLP-logs export, with the protobuf note and resource attributes. Endpoint pattern
`ingress.<coralogix-domain>:443`, `Authorization: Bearer <send-your-data-api-key>`.

## Verify (close the loop)

```bash
cx logs "filter \$l.applicationname == '<app>'" --start now-15m --limit 5
```

## Common failures → fixes

| Symptom | Likely cause | Fix |
|---|---|---|
| No logs after 10 min | Sent as plain text, not OTLP protobuf | Switch exporter to OTLP protobuf |
| Wrong app/subsystem | Attributes unset | Set `cx.application.name` / `cx.subsystem.name` |
| *TODO* | | |

## Tier & cost / AI layers / Docs deep-links

*TODO (Logs PM)* — follow the template. Docs: [OpenTelemetry custom logs](https://coralogix.com/docs/developer-portal/apis/data-ingestion/opentelemetry-custom-logs/),
[Explore logs](https://coralogix.com/docs/user-guides/data_exploration/logs/quickstart/),
[endpoints](https://coralogix.com/docs/integrations/coralogix-endpoints/).

## Sources / evidence

The protobuf prerequisite is a well-known real-world onboarding failure. Fill remaining sections from
the Coralogix logs docs + real cases.
