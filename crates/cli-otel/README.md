# cli-otel

A Rust library for tracing short-lived CLI invocations. It starts an OpenTelemetry
`INTERNAL` span, runs the command future in its context and records the process
exit code. The application supplies the endpoint, auth headers, resource fields
and an exit classifier.

The optional `clap` adapter builds sanitized `process.command_args` from the
declared executable, canonical subcommand path and explicitly passed flag names.
It uses the long flag name when one is declared and falls back to the short
name. The array uses that canonical order independently of the original token
order. Its sources are parser metadata and declarations. Argument values,
positionals, defaults, environment-sourced options and external subcommand
names are excluded.

## Adoption

With the `clap` feature, pass a configured parser and your dispatch to
`run_clap`. The wrapper parses once, records metadata and attempts export before
returning. A process entry point can call `exit()` on `ClapError::Parse` to
keep clap's output streams and exit codes for help, version and usage errors.
Run the async wrapper from a Tokio runtime with I/O and timers enabled.

```rust
use cli_otel::{run_clap, ClapError, Config, KeyValue, Outcome, ParentTrace, Telemetry};

async fn execute(config: Option<Config>) -> Result<(), ClapError<std::io::Error>> {
    let parser = clap::Command::new("tool").subcommand(clap::Command::new("check"));
    run_clap(
        config.and_then(Telemetry::new),
        ParentTrace::default(),
        parser,
        std::env::args_os(),
        || cli_otel::record_attribute(KeyValue::new("myapp.checks.enabled", true)),
        |matches| async move {
            // Dispatch the command described by matches.
            let _ = matches;
            Ok(())
        },
        |result| Outcome::completed(if result.is_ok() { 0 } else { 1 }),
    )
    .await
}
```

Create a `Config` with the full OTLP/HTTP traces URL, declared executable name
and version. Apply the application's opt-out policy before constructing a
configuration that reads installation identity or other system metadata. The
library accepts explicit `TRACEPARENT` and `TRACESTATE` input through
`ParentTrace`. Trace context propagates across asynchronous polls. Sampling
is explicitly always-on once configured, including when an incoming parent is
unsampled.

The pre-dispatch and outcome callbacks run in the command trace. Use
`record_attribute` wherever your application determines a safe bounded value,
such as the effective output format after configuration resolution. Other
parsers can construct `CommandMetadata` from their declarations and use
`Telemetry::run` followed by `Telemetry::shutdown`. For other child operations,
use `start_child_span` to add safe attributes and end the span. Wrap any spawned
child futures with `with_current_context` and await them before shutdown.

## HTTP client spans

Enable `reqwest-middleware` to use the `HttpMiddleware` adapter with reqwest
0.13. Register the adapter once when building a shared client.

```rust
use cli_otel::{HttpMiddleware, KeyValue};
use reqwest_middleware::ClientBuilder;

let client = ClientBuilder::new(reqwest::Client::new())
    .with(HttpMiddleware::new(|request| {
        if request.url().path() == "/api/health" {
            vec![KeyValue::new("url.template", "/api/health")]
        } else {
            Vec::new()
        }
    }))
    .build();
```

Requests through the registered client automatically create client spans in the
active CLI trace. The callback runs only during an active invocation. Return
declared route templates and other bounded, safe attributes. By default, the
adapter records the method, request host and explicit port, response code and a
bounded failure category. Use `with_server_address(false)` when hostnames can
identify customer environments. The attribute callback can supply a recognized
address and port. The adapter forwards the body as a stream and ends the span
when reading completes, fails or the body is dropped.

## Fields and lifecycle

The execution span name is the declared executable followed by the canonical
command path when available. `process.interactive` checks whether stdin is
attached to a terminal. A zero exit code leaves OTel status unset. A nonzero exit
sets error status and a low-cardinality `error.type`. To classify explicit
cancellation, return an `Outcome` with `Termination::Cancelled` and the
application's actual exit code.

The [OpenTelemetry CLI conventions](https://opentelemetry.io/docs/specs/semconv/cli/cli-spans/)
are in development. Standard OTel names used by the core are
`service.name`, `service.version`, `process.executable.name`, `process.pid`,
`process.exit.code`, `process.command_args`, `process.interactive` and `error.type`. HTTP middleware uses
standard HTTP attributes: `http.request.method`, `http.response.status_code`,
`server.address` and `server.port`. Applications may supply a known
`url.template` through the HTTP attribute callback.

The following generic fields are proposed extensions, not standardized CLI
attributes. Applications own the meaning and safety of their additional fields.

| Proposed generic extension | Meaning | Supplied by |
| --- | --- | --- |
| `cli.mode` | Invocation lifecycle, currently `oneshot` | Library |
| `cli.termination.reason` | `completed`, `cancelled`, `timeout` or `signal` | Outcome classifier |
| `cli.installation.method` | Known installation channel, or `unknown` | Application |
| `cli.output.format` | Effective output format | Application |
| `cli.ci.provider` | Recognized CI provider | Application |

Application-specific extensions use their own namespace, such as `cx.cli.*`.

The current wrapper implements execution/callee spans, not spans representing a
parent process launching another CLI. Its normally finalized spans include the
required executable name, PID and exit code. Successful parses use the documented
canonical-path naming format permitted by the CLI convention. Help and unsuccessful
parses record only the declared executable in `process.command_args`. The library
omits workstation executable paths.
The built-in error categories are `argument_parse` and fallback `error`;
applications supply and document their bounded categories through `Outcome`.
The explicit parent-environment mapping belongs to the application contract,
since the CLI specification currently leaves propagation unspecified.

The private provider keeps up to 256 completed spans. Command paths have up to
eight components with at most 64 bytes per name. The library caps
`process.command_args` at 32 entries, retaining the command path first. It bounds attribute keys, string values, arrays and the
total encoded request size. Status descriptions are omitted. Limit application
attributes to values approved for telemetry, such as declared labels and bounded
counts.

## Delivery

`Config::new` selects `Delivery::Inline`. At shutdown, the library sends one
OTLP/HTTP protobuf trace request to the configured URL. The deadline defaults
to one second. The export client connects directly and skips redirects.
The upstream `opentelemetry-otlp` span exporter serializes finalized spans for
inline delivery and the local spool. Its custom HTTP client uses the explicit
destination and headers. Upstream 0.32 merges environment headers and has no
explicit uncompressed setting, so cli-otel discards the exporter's HTTP
headers and selects gzip internally to override ambient compression. The
adapter bounds and decompresses that body before sending or storing ordinary
protobuf. The library leaves the process environment untouched.

Enable the `command-failure-logs` feature and set
`config.command_failure_logs = true` to add inline failure logs. The trace URL
must use the exact `/v1/traces` path and have no query or fragment. The library
sends logs to `/v1/logs` on the same origin with the same headers. Both requests
share the configured deadline. The log uses the command span's trace ID, span
ID, completion time and resource. Its `ERROR` severity and `CLI command failed`
body are fixed. Its attributes include the declared executable and sanitized `process.command_args`,
integer exit code, termination reason and bounded error category. Supply safe
resource fields, declared command names and bounded error categories in the
trace configuration and classifier.

Set `config.delivery = Delivery::Spool(Spool::new(directory))` to save the request
locally at shutdown. Use a dedicated private directory per destination. The
queue stores only the OTLP trace body, including its original resource, trace
IDs and timestamps. The caller supplies consent, credentials and transport.
Hold the file returned by `Spool::try_lock` while sending the batches returned by
`Spool::pending`. Call `Spool::remove` once a request is delivered or needs to be
discarded. Other invocations can enqueue while the sender holds its lock.
To export failure logs during replay, apply your application's privacy policy
to the queued trace, then use `encode_command_failure_logs` to derive the OTLP
log body. `command_failure_logs_endpoint` resolves the standard same-origin
logs route. Acknowledge delivery after both required signals succeed. Replaying
a partially delivered batch can duplicate either signal.

The queue keeps at most 128 batches, 8 MiB total, 512 KiB per batch and seven
days of history. It removes invalid and expired files and evicts the oldest
when full. New queue files and directories are private on Unix. The library
silently drops data that exceeds limits or encounters export or queue errors,
leaving the original command result intact.

Normal shutdown requires control to return to the application's wrapper.
`process::exit` inside a command, `panic=abort` and external termination can lose
the span. A full span buffer or request drops older spans.
