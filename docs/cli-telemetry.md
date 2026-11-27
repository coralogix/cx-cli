# cx CLI telemetry (v1)

`cx` uses the Coralogix-owned `cli-otel` crate for one execution span per command. The
generic crate owns OpenTelemetry environment configuration, HTTPS checks, host fields, CI
and caller detection, safe-field filtering, and the one-second default export deadline.
Its optional Coralogix feature supplies the application and subsystem routing values.

## Opt-in policy

Telemetry is disabled by default. Set `CX_TELEMETRY=1` (or another truthy value) to opt
in. To disable it for one command, prefix the command with `DO_NOT_TRACK=1`, for example:

    DO_NOT_TRACK=1 cx schema

To disable it for the current Bash or Zsh session, run `export DO_NOT_TRACK=1`. In
PowerShell, run `$env:DO_NOT_TRACK = "1"`. To keep it disabled in future sessions, add the
variable to your shell profile. `DO_NOT_TRACK` overrides `CX_TELEMETRY` when both are set.
The opt-in is environment-only and is not remembered in `~/.cx/config.toml`. A telemetry
destination must also be configured; cx does not export to the generic library's
localhost default when no destination is set.

## Export destination

The standard `OTEL_EXPORTER_OTLP_*` settings are supported. `OTEL_EXPORTER_OTLP_ENDPOINT`
is a base URL and receives the `/v1/traces` suffix; `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`
is used as the exact traces URL. Traces-specific headers, timeout, protocol, compression,
and certificate settings override general OTLP settings. Only `http/protobuf` is supported.
Remote endpoints must use HTTPS. HTTP is accepted only for loopback; credentials are
removed from loopback HTTP requests.

For cx-only setup, `CX_OTLP_ENDPOINT` is the exact traces URL and `CX_OTLP_API_KEY` is
sent as a Bearer token. The Coralogix application and subsystem headers are supplied by
the library's Coralogix feature. The export timeout defaults to one second and can be
changed with the supported timeout environment variables.

`TRACEPARENT` and `TRACESTATE` are accepted as optional parent context inputs. Export
failures never change the command result or exit status.

## Attributes and privacy

The shared library emits standard process, service, OS, and host fields, plus generic
`cli.ci.provider` and `cli.agent.name` fields when it detects known environments. The cx
integration adds `cx.application.name` and `cx.subsystem.name`, plus Coralogix ingress headers.
The shared library records a detected coding agent with both
`cli.agent.name` and the current `gen_ai.agent.id` semantic-convention key.

No raw command argument values, profile names, query text, request/response bodies, or
credentials are exported.

## Deferred work

There is no detached sender, disk spool, or retry queue. Export is inline, best-effort,
and bounded by the configured deadline.
