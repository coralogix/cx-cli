# cx CLI telemetry (v1)

`cx` uses the Coralogix-owned `cli-otel` crate for one execution span per command,
optional correlated failure logs, and HTTP client child spans. The generic crate owns
OpenTelemetry environment configuration, HTTPS checks, host fields, CI and caller
detection, and the one-second default export deadline. The cx adapter owns consent policy,
Coralogix attributes, ingress headers, and its safe-field allowlist.

## Opt-in policy

Telemetry is disabled by default. Set `CX_TELEMETRY=1` (or another truthy value) to opt
in. `DO_NOT_TRACK` always disables export, even when `CX_TELEMETRY` is enabled. A telemetry
destination must also be configured; cx does not export to the generic library's
localhost default when no destination is set. The opt-in is environment-only and is not
remembered in `~/.cx/config.toml`.

## Export destination

The standard `OTEL_EXPORTER_OTLP_*` settings are supported. `OTEL_EXPORTER_OTLP_ENDPOINT`
is a base URL and receives the `/v1/traces` suffix; `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`
is used as the exact traces URL. Traces-specific headers, timeout, protocol, compression,
and certificate settings override general OTLP settings. Only `http/protobuf` is supported.
Remote endpoints must use HTTPS. HTTP is accepted only for loopback; credentials are
removed from loopback HTTP requests.

For cx-only setup, `CX_OTLP_ENDPOINT` is the exact traces URL and `CX_OTLP_API_KEY` is
sent as a Bearer token. The Coralogix application and subsystem headers are added by the
cx adapter. The export deadline is capped at one second for command responsiveness.
`CX_TELEMETRY_FAILURE_LOGS=1` also exports a correlated failure log to the `/v1/logs`
sibling of the traces endpoint.

`TRACEPARENT` and `TRACESTATE` are accepted as optional parent context inputs. Export
failures never change the command result or exit status.

## Attributes and privacy

The shared library emits standard process, service, OS, and host fields, plus generic
`cli.ci.provider` and `cli.agent.name` fields when it detects known environments. The cx
adapter adds `cx.application.name`, `cx.subsystem.name`, `cx.cli.output.format`,
`cx.cli.command.risky`, `cx.cli.query.language`, `cx.cli.profile.count`, and the Coralogix
region and ingress headers. The shared library records a detected coding agent with both
`cli.agent.name` and the current `gen_ai.agent.id` semantic-convention key.

No raw command argument values, profile names, query text, request/response bodies, or
credentials are exported.

## Deferred work

There is no detached sender, disk spool, or retry queue. Export is inline, best-effort,
and bounded by the configured deadline.
