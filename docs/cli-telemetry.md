# cx CLI telemetry

A configured cx invocation records one command trace. The upstream OpenTelemetry
OTLP span exporter serializes it for OTLP/HTTP protobuf delivery.
Failed commands can also produce one correlated OTLP log.
Release builds attempt background delivery through Coralogix's dedicated
telemetry key-discovery route. Delivery requires that route to return a usable
credential. Debug builds require an explicit endpoint or key URL.

| Setting | Effect |
| --- | --- |
| `telemetry = false` in `~/.cx/config.toml` | Opts out of recording and sending. |
| `CX_TELEMETRY` | Unset, empty, `1`, `true`, `on` and `yes` allow configured collection. Other values, including `0`, `false` and `off`, opt out. |
| `DO_NOT_TRACK` | `1`, `true`, `on` and `yes` opt out. |
| `CX_TELEMETRY_ENDPOINT` | Explicit full HTTPS traces URL. For a local collector, use a loopback address such as `http://127.0.0.1:4318/v1/traces`. An empty value disables export. |
| `CX_TELEMETRY_API_KEY` | Optional bearer token for the explicit endpoint. |
| `CX_TELEMETRY_FAILURE_LOGS=1` | Enable failure logs for an explicit endpoint when its path is `/v1/traces` and it has no query string. Discovered endpoints send failure logs automatically. |
| `CX_TELEMETRY_DELIVERY=background` | Queue telemetry for an explicit endpoint. Explicit endpoints otherwise send inline, with one shared one-second timeout. |
| `CX_TELEMETRY_KEY_URL` | Override the release key-discovery URL when no explicit traces endpoint is set. An empty value disables discovery. |
| `TRACEPARENT` and `TRACESTATE` | Optional W3C parent trace context. |

Consent values are case-insensitive and trimmed. Any opt-out wins, including an
unreadable or invalid config file and a non-boolean `telemetry` setting. cx checks
consent before loading installation identity, queueing and sending.

Before implicit-default collection, a release build prints a first-run stderr
notice with opt-out instructions. A completed marker under
`~/.cx/telemetry-notice/` is saved atomically after the notice is written.
Invalid markers trigger disclosure again. If the marker cannot be saved, the
notice may repeat. If stderr cannot be written, that invocation skips implicit
collection. Explicit consent settings or a configured developer destination do
not need the default notice. The detached sender stays silent.

cx checks consent again before exporting a completed command and at the delivery
boundary. Changing `telemetry` to `false` while a command is running drops its
buffered trace and failure log instead of sending or queueing them. Re-enabling
telemetry later does not recover data dropped by that invocation.

## Background delivery

The default release discovery URL is
`https://api.eu2.coralogix.com/api/v2/cli-telemetry-key`. The sender retrieves a
telemetry key and validates the HTTPS Coralogix ingress host before sending to
`/v1/traces` and `/v1/logs`. It caches the key for 24 hours and refreshes it once
after an HTTP 401 or 403. Explicit traces URLs use their own optional runtime
token. When enabled, the matching explicit logs URL uses the same origin and
token. Custom traces paths or traces URLs with a query string keep trace-only
delivery.
Both delivery modes are independent of Coralogix profile credentials and
`OTEL_*` environment variables.

With background delivery, cx writes the finished trace under
`~/.cx/telemetry/` and launches a detached sender after the command. A separate
private directory for each destination holds up to 128 batches and 8 MiB, with
a 512 KiB limit per batch and a seven-day replay window. Trace IDs, span IDs,
timestamps and resource attributes stay with the original batch.

The sender checks consent before reading credentials and before sending or
retrying each signal. It validates the queued trace against cx's privacy policy
before deriving its log. The sender posts the trace first and removes the batch
as delivered after the trace and any required log receive successful HTTP
responses. A permanent rejection discards the batch. A transient failure keeps
it in the queue for retry.
Older queued traces retain their original fields. The sender checks their
command and flags against the current CLI before deriving sanitized arguments
for a failure log.
It exits after at most 30 seconds of delivery work. A destination that remains
unavailable gets a one-minute cooldown before a later invocation retries.
Retries can duplicate either signal, including a trace that succeeded before
its log failed. For an explicit queue, the sender uses the current failure-log
setting when replaying. To discard queued batches and cached credentials, opt
out first and remove `~/.cx/telemetry/`.

## Correlated failure logs

Each finalized command with a nonzero exit emits one `ERROR` record when logs
are enabled. The body is `CLI command failed`. The record keeps the command
span's trace ID, span ID, completion time and original resource. Its attributes
are `process.executable.name`, `process.exit.code`, `cli.termination.reason`,
`error.type` and sanitized `process.command_args`.
The exit code is an integer. Logs exclude raw error messages, stderr, argument
values and HTTP child diagnostics. HTTP children can fail independently of the
command's exit outcome. Abrupt exits that bypass command finalization can lose
the record.

## Trace fields

Command spans record the executable, canonical command and explicit flag names
as sanitized `process.command_args`. They also
capture the effective output format, resolved profile count, query language and
risky write classification when the command reaches those decisions. The resource
contains cx version, OS type and available numeric version, host architecture, locally
stored installation UUID and detected install method. Current installation
detection distinguishes Homebrew, install script and unknown. The installation
UUID uses the standard OpenTelemetry application attribute.
[OpenTelemetry application attributes](https://opentelemetry.io/docs/specs/semconv/registry/attributes/app/)

Known CI providers are identified by their standard boolean environment markers.
Known agent callers get a normalized `gen_ai.agent.name` from cx's caller
detection. The OS and agent fields describe the invocation environment.
`process.interactive` reflects terminal attachment on stdin. cx traces
oneshot commands. Shell completion's internal `COMPLETE` protocol is
handled separately before telemetry initialization.

The cx entry point selects its legacy profiles or regular clap parser once and
passes it to `cli_otel::run_clap`. The wrapper owns parsing, trace metadata,
outcome recording and export. The cx adapter supplies caller and CI metadata,
plus the command's query language and risky-write classification. The dispatch
reports the effective output format once it resolves flags and configuration,
and the selected profile count once credentials resolve.

Shared Coralogix API requests produce child spans with method, response status
when available and region for recognized Coralogix hosts. cx exports server
addresses only for known Coralogix API hosts and local test addresses. The
shared client registers `HttpMiddleware` once. Its response spans include the
time spent streaming the body and finalize on completion, read error or drop.
Only reviewed literal route templates are included. Argument and flag values, profile
names, query text, URL paths with identifiers, request and response bodies, auth,
and raw error messages are excluded. `error.type` is a bounded category or
an HTTP status code. Explicit cancellation from a prompt retains its original
exit behavior. OS signals can terminate the process before export.

## Standard attributes and extensions

Standard names come from existing OpenTelemetry conventions. This does not imply
that every attribute is stable or defined specifically by the CLI convention.
The additions below remain application/instrumentation conventions.

| Classification | Attributes |
| --- | --- |
| Standard resource attributes | `service.name`, `service.version`, `os.type`, `os.version`, `host.arch`, `app.installation.id` |
| Standard process/error attributes | `process.executable.name`, `process.pid`, `process.exit.code`, `process.command_args`, `process.interactive`, `error.type` |
| Standard HTTP attributes | `http.request.method`, `http.response.status_code`, `server.address`, `server.port`, `url.template` |
| Proposed generic CLI extensions | `cli.mode`, `cli.termination.reason`, `cli.installation.method`, `cli.output.format`, `cli.ci.provider` |
| cx-specific extensions | `cx.cli.profile.count`, `cx.cli.query.language`, `cx.cli.command.risky`, `cx.cli.region` |
| cx resource fields for discovered delivery | `cx.application.name=cx-cli`, `cx.subsystem.name=usage` |

`gen_ai.agent.name` is an existing GenAI attribute name. Reusing it to identify
cx's agent caller is an application convention; the CLI span specification does
not prescribe that use. A `TRACEPARENT`/`TRACESTATE` environment carrier is also
an application/library contract.

## CLI semantic convention alignment

The reference is the upstream [CLI spans specification](https://raw.githubusercontent.com/open-telemetry/semantic-conventions/refs/heads/main/docs/cli/cli-spans.md), checked on 2026-11-23. It is a Development convention for short-lived programs.

cx implements execution (callee) spans. The root is `INTERNAL`; HTTP children use the separate HTTP client conventions. The documented root name is `cx` followed by the canonical declared command path when parsing succeeds, such as `cx logs` or `cx profiles list`. Help and unsuccessful parsing retain `cx`. Names exclude values and external subcommand names.

Every normally completed invocation records `process.executable.name`, `process.pid` and its actual `process.exit.code`. Nonzero exits have error status and `error.type`; success leaves status unset and omits `error.type`. Explicit prompt cancellation keeps its nonzero exit and is therefore an error under this convention, with `cli.termination.reason=cancelled` providing a separate classification. Abrupt termination can prevent finalization, as described above.

The recommended `process.command_args` contains only the declared executable,
canonical subcommand path and explicitly passed flag names. For example,
`["cx", "profiles", "list", "--label"]` omits the label value. The array uses
that canonical order independently of the original token order. Help and
unsuccessful parses record `["cx"]`. cx omits the executable path and all
argument values. The proposed `cli.*` fields and application-owned `cx.cli.*`
fields are extensions. The library accepts `TRACEPARENT` and `TRACESTATE`
as an explicit propagation contract.

The command error categories are `argument_parse`, `configuration`, `profile_selection`, `authentication`, `permission`, `api`, `invalid_argument`, `timeout`, `connection`, `http`, `json`, `io`, `query_stream`, `cancelled` and fallback `error`. The generic runner supplies `argument_parse` for unsuccessful parsing and fallback `error` for an unclassified nonzero outcome. HTTP spans use a response status code as their error type for unsuccessful responses, or `timeout`, `connection` or `http` for transport/read failures. Messages and queries are never error categories.

Run the local verification script from the checkout.

```sh
scripts/demo-cli-telemetry.sh
```

The tests start loopback mock servers, run cx commands and decode exported
protobuf. They cover command and HTTP spans, failure log correlation, parent
propagation, consent, privacy, bounded queues, background replay and key
rotation.

The reusable [`cli-otel` facade crate](../crates/cli-otel/README.md) documents
the standard attributes and generic extensions. The generic implementation
source is bundled once under `src/cli_otel/` in this repository and compiled
directly into `coralogix-cli` for release packaging. The local
`crates/cli-otel` package is `publish = false` and points its library path at
that shared source for workspace reuse and focused library tests.
