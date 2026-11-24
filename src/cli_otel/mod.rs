//! Record and export one short-lived CLI invocation with a private OpenTelemetry provider.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use opentelemetry::trace::{Span, SpanKind, Status};
pub use opentelemetry::{Array, KeyValue, Value};

use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::{FutureExt, TraceContextExt, Tracer, TracerProvider};
use opentelemetry::{Context, InstrumentationScope};
#[cfg(all(test, not(cli_otel_embedded)))]
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{
    Sampler, SdkTracer, SdkTracerProvider, SpanData, SpanExporter, SpanProcessor,
};
use opentelemetry_sdk::Resource;
pub use reqwest::header;
pub use reqwest::Url;
use reqwest::{redirect::Policy, Client, ClientBuilder};
use rustls_platform_verifier::BuilderVerifierExt;

mod exporter;
mod limits;
mod otlp;
mod spool;

#[cfg(any(feature = "command-failure-logs", cli_otel_embedded))]
mod failure_logs;
#[cfg(any(feature = "command-failure-logs", cli_otel_embedded))]
pub use failure_logs::{command_failure_logs_endpoint, encode_command_failure_logs};

use limits::{
    bound_initial_child_attributes, bound_resource_attributes, bound_span, bounded_text, flag,
    label, valid_attribute, MAX_ARRAY_ITEMS, MAX_COMMAND_DEPTH, MAX_FLAGS, MAX_SPANS,
    MAX_TRACE_STATE_BYTES, MAX_VALUE_BYTES, SDK_LIMITS,
};
pub use spool::{PendingBatch, Spool};
const DEFAULT_EXPORT_TIMEOUT: Duration = Duration::from_secs(1);

#[cfg(any(feature = "clap", cli_otel_embedded))]
mod clap_runner;
#[cfg(any(feature = "clap", cli_otel_embedded))]
pub use clap_runner::{run_clap, ClapError};

#[cfg(any(feature = "reqwest-middleware", cli_otel_embedded))]
mod http_middleware;
#[cfg(any(feature = "reqwest-middleware", cli_otel_embedded))]
pub use http_middleware::HttpMiddleware;

/// Export settings supplied by the application. The library does not resolve
/// endpoints, headers, resources, sampling or limits from environment variables.
pub struct Config {
    /// Full OTLP/HTTP traces URL, including its path.
    pub endpoint: Url,
    /// OTLP/HTTP headers used for inline export.
    pub headers: header::HeaderMap,
    /// Deadline for inline export.
    pub export_timeout: Duration,
    /// Export a correlated failure log inline when the endpoint has a standard logs sibling.
    #[cfg(any(feature = "command-failure-logs", cli_otel_embedded))]
    pub command_failure_logs: bool,
    pub delivery: Delivery,
    /// Optional application-owned consent predicate checked before exporting
    /// buffered spans and again at the transport boundary.
    pub delivery_allowed: Option<DeliveryAllowed>,
    /// Declared executable name, without a filesystem path.
    pub executable_name: &'static str,
    pub service_version: &'static str,
    /// Application-owned resource fields. Supply only values safe to export.
    pub resource_attributes: Vec<KeyValue>,
}

/// Select inline HTTP export or queueing for an application-owned sender.
#[derive(Clone, Default)]
pub enum Delivery {
    #[default]
    Inline,
    Spool(Spool),
}

impl Config {
    pub fn new(
        endpoint: Url,
        executable_name: &'static str,
        service_version: &'static str,
    ) -> Self {
        Self {
            endpoint,
            headers: header::HeaderMap::new(),
            export_timeout: DEFAULT_EXPORT_TIMEOUT,
            #[cfg(any(feature = "command-failure-logs", cli_otel_embedded))]
            command_failure_logs: false,
            delivery: Delivery::Inline,
            delivery_allowed: None,
            executable_name,
            service_version,
            resource_attributes: Vec::new(),
        }
    }
}

pub type DeliveryAllowed = Arc<dyn Fn() -> bool + Send + Sync>;

/// Application-declared command metadata. Use the canonical names of declared
/// subcommands and flags. Do not include argument values or argv fragments.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CommandMetadata {
    pub command_path: Vec<String>,
    pub flags: Vec<String>,
}

impl CommandMetadata {
    /// Extract declared flag and canonical command names from a successful
    /// clap parse. Defaults, environment-sourced options and positionals are omitted.
    #[cfg(any(feature = "clap", cli_otel_embedded))]
    pub fn from_clap(command: &clap::Command, matches: &clap::ArgMatches) -> Self {
        use clap::parser::ValueSource;
        use std::collections::BTreeSet;

        let mut command_path = Vec::new();
        let mut flags = BTreeSet::new();
        let mut command = command;
        let mut matches = matches;

        loop {
            for id in matches.ids() {
                if flags.len() == MAX_FLAGS {
                    break;
                }
                if matches.value_source(id.as_str()) != Some(ValueSource::CommandLine) {
                    continue;
                }
                if let Some(arg) = command.get_arguments().find(|arg| arg.get_id() == id) {
                    let name = if let Some(name) = arg.get_long().filter(|name| label(name)) {
                        Some(format!("--{name}"))
                    } else {
                        arg.get_short().map(|short| format!("-{short}"))
                    };
                    if let Some(name) = name.filter(|name| flag(name)) {
                        flags.insert(name);
                    }
                }
            }

            let Some((name, nested_matches)) = matches.subcommand() else {
                break;
            };
            let Some(nested_command) = command.find_subcommand(name) else {
                // External subcommand names are supplied by argv.
                break;
            };
            let canonical_name = nested_command.get_name();
            if command_path.len() == MAX_COMMAND_DEPTH || !label(canonical_name) {
                break;
            }
            command_path.push(canonical_name.to_owned());
            command = nested_command;
            matches = nested_matches;
        }

        Self {
            command_path,
            flags: flags.into_iter().collect(),
        }
    }
}

/// Explicit W3C trace inputs. Only trace context is extracted.
#[derive(Default)]
pub struct ParentTrace<'a> {
    pub traceparent: Option<&'a str>,
    pub tracestate: Option<&'a str>,
}

impl ParentTrace<'_> {
    fn context(&self) -> Context {
        TraceContextPropagator::new().extract_with_context(&Context::new(), self)
    }
}

impl Extractor for ParentTrace<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "traceparent" => self
                .traceparent
                .filter(|value| bounded_text(value, MAX_TRACE_STATE_BYTES)),
            "tracestate" => self
                .tracestate
                .filter(|value| bounded_text(value, MAX_TRACE_STATE_BYTES)),
            _ => None,
        }
    }

    fn keys(&self) -> Vec<&str> {
        vec!["traceparent", "tracestate"]
    }
}

#[derive(Clone, Copy)]
pub enum Termination {
    Completed,
    Cancelled,
    Timeout,
    Signal,
}

impl Termination {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::Signal => "signal",
        }
    }
}

/// Classification of the application's original result.
pub struct Outcome {
    pub exit_code: i32,
    pub termination: Termination,
    /// Bounded application error category. A nonzero exit defaults to "error".
    pub error_type: Option<&'static str>,
}

impl Outcome {
    pub fn completed(exit_code: i32) -> Self {
        Self {
            exit_code,
            termination: Termination::Completed,
            error_type: None,
        }
    }
}

struct ActiveCommand {
    executable_name: &'static str,
    tracer: SdkTracer,
    command_args: Mutex<Vec<String>>,
    attributes: Mutex<Vec<KeyValue>>,
}

#[derive(Clone, Default)]
struct SpanBuffer(Arc<Mutex<VecDeque<SpanData>>>);

impl fmt::Debug for SpanBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("SpanBuffer").finish_non_exhaustive()
    }
}

impl SpanBuffer {
    fn drain(&self) -> Vec<SpanData> {
        self.0
            .lock()
            .map_or_else(|_| Vec::new(), |mut spans| spans.drain(..).collect())
    }
}

impl SpanProcessor for SpanBuffer {
    fn on_start(&self, _: &mut opentelemetry_sdk::trace::Span, _: &Context) {}

    fn on_end(&self, mut span: SpanData) {
        bound_span(&mut span);
        if let Ok(mut spans) = self.0.lock() {
            if spans.len() == MAX_SPANS {
                spans.pop_front();
            }
            spans.push_back(span);
        }
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        Ok(())
    }
}

/// A CLI invocation with a private tracer and a bounded buffer of finished spans.
pub struct Telemetry {
    executable_name: &'static str,
    exporter: opentelemetry_otlp::SpanExporter,
    provider: SdkTracerProvider,
    buffer: SpanBuffer,
    delivery_allowed: Option<DeliveryAllowed>,
}

impl Telemetry {
    /// An invalid endpoint or unavailable transport disables telemetry.
    pub fn new(mut config: Config) -> Option<Self> {
        if !label(config.executable_name)
            || !bounded_text(config.service_version, MAX_VALUE_BYTES)
            || !matches!(config.endpoint.scheme(), "http" | "https")
            || config.endpoint.host().is_none()
            || !config.endpoint.username().is_empty()
            || config.endpoint.password().is_some()
        {
            return None;
        }

        let mut attributes = std::mem::take(&mut config.resource_attributes);
        bound_resource_attributes(&mut attributes);
        let resource = Resource::builder_empty()
            .with_attributes(attributes)
            .with_service_name(config.executable_name)
            .with_attributes([KeyValue::new("service.version", config.service_version)])
            .build();
        let executable_name = config.executable_name;
        let delivery_allowed = config.delivery_allowed.clone();
        let exporter = exporter::build(config, &resource)?;
        let buffer = SpanBuffer::default();
        let provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_sampler(Sampler::AlwaysOn)
            .with_span_limits(SDK_LIMITS)
            .with_span_processor(buffer.clone())
            .build();

        Some(Self {
            executable_name,
            exporter,
            provider,
            buffer,
            delivery_allowed,
        })
    }

    /// Run an invocation and record its classified outcome. The original result
    /// is returned unchanged. Application enrichment can occur inside the future.
    pub async fn run<F, T>(
        &self,
        metadata: CommandMetadata,
        parent: ParentTrace<'_>,
        future: F,
        classify: impl FnOnce(&T) -> Outcome,
    ) -> T
    where
        F: Future<Output = T>,
    {
        let executable_name = self.executable_name;
        let tracer = self.provider.tracer_with_scope(
            InstrumentationScope::builder("cli-otel")
                .with_version(env!("CARGO_PKG_VERSION"))
                .build(),
        );
        let parent = parent.context();
        let span = tracer
            .span_builder(executable_name)
            .with_kind(SpanKind::Internal)
            .with_attributes([
                KeyValue::new("process.executable.name", executable_name),
                KeyValue::new("process.pid", i64::from(std::process::id())),
                KeyValue::new("process.interactive", std::io::stdin().is_terminal()),
                KeyValue::new("cli.mode", "oneshot"),
            ])
            .start_with_context(&tracer, &parent);
        let context = parent.with_span(span).with_value(ActiveCommand {
            executable_name,
            tracer,
            command_args: Mutex::new(Vec::new()),
            attributes: Mutex::new(Vec::new()),
        });

        set_metadata(&context, || metadata);
        let (result, outcome) = async {
            let result = future.await;
            let outcome = classify(&result);
            (result, outcome)
        }
        .with_context(context.clone())
        .await;
        let span = context.span();
        span.set_attribute(KeyValue::new(
            "process.exit.code",
            i64::from(outcome.exit_code),
        ));
        span.set_attribute(KeyValue::new(
            "cli.termination.reason",
            outcome.termination.as_str(),
        ));
        if outcome.exit_code != 0 {
            span.set_attribute(KeyValue::new(
                "error.type",
                outcome
                    .error_type
                    .filter(|value| label(value))
                    .unwrap_or("error"),
            ));
            span.set_status(Status::error(""));
        }
        if let Some(active) = context.get::<ActiveCommand>() {
            if let Ok(mut command_args) = active.command_args.lock() {
                span.set_attribute(KeyValue::new(
                    "process.command_args",
                    Value::Array(Array::String(
                        command_args.drain(..).map(Into::into).collect(),
                    )),
                ));
            }
            if let Ok(mut attributes) = active.attributes.lock() {
                span.set_attributes(attributes.drain(..));
            }
        }
        span.end();

        result
    }

    /// Close the provider and export the finished telemetry or enqueue it locally.
    /// Complete application child tasks before calling this method.
    pub async fn shutdown(self) {
        let _ = self.provider.shutdown();
        let spans = self.buffer.drain();
        if !spans.is_empty() && self.delivery_allowed() {
            let _ = self.exporter.export(spans).await;
        }
    }

    fn delivery_allowed(&self) -> bool {
        self.delivery_allowed
            .as_ref()
            .is_none_or(|allowed| allowed())
    }
}

pub(crate) fn http_client_builder(endpoint: &Url) -> Option<ClientBuilder> {
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .ok()?
    .with_platform_verifier()
    .ok()
    .map(|builder| builder.with_no_client_auth());

    let mut builder = Client::builder().redirect(Policy::none());
    match tls {
        Some(tls) => {
            builder = builder.tls_backend_preconfigured(tls);
        }
        None if endpoint.scheme() == "http" => {
            // Allow cleartext HTTP when the platform trust store is unavailable.
            // The empty root store ensures any TLS peer or HTTPS proxy trust check fails closed.
            let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .ok()?
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
            builder = builder.tls_backend_preconfigured(tls);
        }
        None => return None,
    }
    Some(builder)
}

#[cfg(all(test, not(cli_otel_embedded)))]
fn inline_client(timeout: Duration) -> Option<Client> {
    let endpoint = Url::parse("https://localhost").ok()?;
    inline_client_for_endpoint(timeout, &endpoint)
}

fn inline_client_for_endpoint(timeout: Duration, endpoint: &Url) -> Option<Client> {
    http_client_builder(endpoint)?.timeout(timeout).build().ok()
}

/// Update command metadata after a parser runs inside the command future.
/// The closure is only called when telemetry is active.
pub fn record_command_metadata(metadata: impl FnOnce() -> CommandMetadata) {
    set_metadata(&Context::current(), metadata);
}

fn set_metadata(context: &Context, metadata: impl FnOnce() -> CommandMetadata) {
    let Some(active) = context.get::<ActiveCommand>() else {
        return;
    };
    let metadata = metadata();
    let mut command_args = vec![active.executable_name.to_owned()];
    command_args.extend(
        metadata
            .command_path
            .into_iter()
            .take(MAX_COMMAND_DEPTH)
            .take_while(|name| label(name)),
    );
    context.span().update_name(command_args.join(" "));
    let remaining = MAX_ARRAY_ITEMS - command_args.len();
    command_args.extend(
        metadata
            .flags
            .into_iter()
            .filter(|name| flag(name))
            .take(remaining.min(MAX_FLAGS)),
    );
    if let Ok(mut args) = active.command_args.lock() {
        *args = command_args;
    }
}

/// Record an application-owned attribute on the active command span.
pub fn record_attribute(attribute: KeyValue) {
    let context = Context::current();
    let Some(active) = context.get::<ActiveCommand>() else {
        return;
    };
    if valid_attribute(&attribute) {
        if let Ok(mut attributes) = active.attributes.lock() {
            if attributes.len() < limits::MAX_ATTRIBUTES {
                attributes.push(attribute);
            }
        }
    }
}

/// Create an application span using the CLI invocation's provider and the
/// current context as its parent. Its caller controls the child span lifetime.
pub fn start_child_span(
    name: &'static str,
    kind: SpanKind,
    attributes: impl FnOnce() -> Vec<KeyValue>,
) -> Option<impl Span + 'static> {
    let context = Context::current();
    let active = context.get::<ActiveCommand>()?;
    let mut attributes = attributes();
    bound_initial_child_attributes(&mut attributes);
    Some(
        active
            .tracer
            .span_builder(name)
            .with_kind(kind)
            .with_attributes(attributes)
            .start_with_context(&active.tracer, &context),
    )
}

/// Carry the current trace context into a future that will be spawned.
pub fn with_current_context<F: Future>(future: F) -> impl Future<Output = F::Output> {
    future.with_current_context()
}

#[cfg(all(
    test,
    not(cli_otel_embedded),
    any(feature = "clap", feature = "reqwest-middleware")
))]
#[path = "../../crates/cli-otel/src/hook_tests.rs"]
mod hook_tests;
#[cfg(all(test, not(cli_otel_embedded)))]
#[path = "../../crates/cli-otel/src/production_tests.rs"]
mod production_tests;
#[cfg(all(test, not(cli_otel_embedded)))]
#[path = "../../crates/cli-otel/src/tests.rs"]
mod tests;
