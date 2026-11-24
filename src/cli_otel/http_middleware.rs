use std::pin::Pin;
use std::task::{Context, Poll};

use http::Extensions;
use http_body::{Body, Frame, SizeHint};
use reqwest::{Request, Response, ResponseBuilderExt};
use reqwest_middleware::{Middleware, Next, Result};

use super::{start_child_span, KeyValue, Span, SpanKind, Status, Value};

/// Records HTTP client spans inside an active CLI invocation.
///
/// Register once with `reqwest_middleware::ClientBuilder::with`. The callback
/// runs only when recording a span. Return bounded fields such as a declared
/// route template or region. The middleware itself records method, host,
/// explicit port, response status and a bounded error type.
pub struct HttpMiddleware<F> {
    attributes: F,
    server_address: bool,
}

impl<F> HttpMiddleware<F>
where
    F: Fn(&Request) -> Vec<KeyValue>,
{
    pub fn new(attributes: F) -> Self {
        Self {
            attributes,
            server_address: true,
        }
    }

    /// Set whether to record the request host and explicit port automatically.
    /// Disable this if the application resolves tenant-specific hosts and can
    /// supply a bounded alternative through its attribute callback.
    pub fn with_server_address(mut self, enabled: bool) -> Self {
        self.server_address = enabled;
        self
    }
}

#[async_trait::async_trait]
impl<F> Middleware for HttpMiddleware<F>
where
    F: Fn(&Request) -> Vec<KeyValue> + Send + Sync + 'static,
{
    async fn handle(
        &self,
        request: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let method = match request.method().as_str() {
            "CONNECT" => "CONNECT",
            "DELETE" => "DELETE",
            "GET" => "GET",
            "HEAD" => "HEAD",
            "OPTIONS" => "OPTIONS",
            "PATCH" => "PATCH",
            "POST" => "POST",
            "PUT" => "PUT",
            "TRACE" => "TRACE",
            _ => "_OTHER",
        };
        let name = if method == "_OTHER" { "HTTP" } else { method };
        let span = start_child_span(name, SpanKind::Client, || {
            let mut attributes = vec![KeyValue::new("http.request.method", method)];
            if self.server_address {
                if let Some(host) = request.url().host_str() {
                    attributes.push(KeyValue::new("server.address", host.to_owned()));
                }
                if let Some(port) = request.url().port() {
                    attributes.push(KeyValue::new("server.port", i64::from(port)));
                }
            }
            attributes.extend((self.attributes)(&request).into_iter().filter(|attribute| {
                !matches!(
                    attribute.key.as_str(),
                    "http.request.method" | "http.response.status_code" | "error.type"
                )
            }));
            attributes
        });

        let Some(mut span) = span else {
            return next.run(request, extensions).await;
        };
        match next.run(request, extensions).await {
            Ok(response) => Ok(trace_body(response, span)),
            Err(error) => {
                let error_type = if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "connection"
                } else {
                    "http"
                };
                record_error(&mut span, error_type);
                span.end();
                Err(error)
            }
        }
    }
}

fn trace_body<S: Span + Send + Sync + Unpin + 'static>(
    response: Response,
    mut span: S,
) -> Response {
    let status = response.status();
    span.set_attribute(KeyValue::new(
        "http.response.status_code",
        i64::from(status.as_u16()),
    ));
    if !status.is_success() {
        record_error(&mut span, status.as_u16().to_string());
    }

    // reqwest preserves the HTTP parts during conversion, but keeps the
    // request URL separately. Restore it through its response builder hook.
    let url = response.url().clone();
    let response: http::Response<reqwest::Body> = response.into();
    let (mut parts, body) = response.into_parts();
    let url_marker = http::Response::builder()
        .url(url)
        .body(())
        .expect("a response builder with only a URL extension is valid");
    parts
        .extensions
        .extend(url_marker.into_parts().0.extensions);

    let body = if body.is_end_stream() {
        span.end();
        body
    } else {
        reqwest::Body::wrap(TracedBody {
            body,
            span: Some(span),
            success: status.is_success(),
        })
    };
    http::Response::from_parts(parts, body).into()
}

fn record_error(span: &mut impl Span, error_type: impl Into<Value>) {
    span.set_attribute(KeyValue::new("error.type", error_type));
    span.set_status(Status::error(""));
}

struct TracedBody<S: Span> {
    body: reqwest::Body,
    span: Option<S>,
    success: bool,
}

impl<S: Span + Unpin> Body for TracedBody<S> {
    type Data = <reqwest::Body as Body>::Data;
    type Error = reqwest::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let frame = Pin::new(&mut self.body).poll_frame(cx);
        if let Poll::Ready(frame) = &frame {
            let failure = frame.as_ref().and_then(|result| result.as_ref().err());
            if frame.is_none() || failure.is_some() || self.body.is_end_stream() {
                if let Some(mut span) = self.span.take() {
                    if let Some(error) = failure.filter(|_| self.success) {
                        record_error(
                            &mut span,
                            if error.is_timeout() {
                                "timeout"
                            } else {
                                "http"
                            },
                        );
                    }
                    span.end();
                }
            }
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

impl<S: Span> Drop for TracedBody<S> {
    fn drop(&mut self) {
        if let Some(mut span) = self.span.take() {
            span.end();
        }
    }
}
