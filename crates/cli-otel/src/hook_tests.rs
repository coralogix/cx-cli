use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

async fn listener() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    (listener.local_addr().unwrap(), listener)
}

async fn receive(listener: &TcpListener) -> (BufReader<TcpStream>, String, usize) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut stream = BufReader::new(stream);
    let mut request = String::new();
    assert!(stream.read_line(&mut request).await.unwrap() > 0);
    let mut length = 0;
    loop {
        let mut header = String::new();
        assert!(stream.read_line(&mut header).await.unwrap() > 0);
        if header == "\r\n" {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap();
            }
        }
    }
    (stream, request, length)
}

#[cfg(feature = "clap")]
mod clap_adopter {
    use std::io;

    use prost::Message;
    use tokio::io::AsyncReadExt;

    use crate::{record_attribute, run_clap, ClapError, Config, KeyValue, Outcome, ParentTrace};

    use super::*;

    async fn run_tool(config: Config, argv: &[&str]) -> Result<i32, ClapError<io::Error>> {
        let command = clap::Command::new("second-tool")
            .version("1.0.0")
            .subcommand(
                clap::Command::new("check")
                    .alias("verify")
                    .arg(clap::Arg::new("name").long("name").required(true)),
            );
        run_clap(
            crate::Telemetry::new(config),
            ParentTrace::default(),
            command,
            argv.iter().copied(),
            || record_attribute(KeyValue::new("example.pre", true)),
            |matches| async move {
                assert_eq!(
                    matches
                        .subcommand_matches("check")
                        .unwrap()
                        .get_one::<String>("name")
                        .unwrap(),
                    "private-value",
                );
                Ok(17)
            },
            |result| {
                record_attribute(KeyValue::new("example.post", true));
                Outcome::completed(*result.as_ref().unwrap())
            },
        )
        .await
    }

    #[tokio::test]
    async fn another_clap_app_uses_one_wrapper_for_dispatch_and_parser_exits() {
        for (args, name, code, dispatched) in [
            (
                ["second-tool", "verify", "--name", "private-value"].as_slice(),
                "second-tool check",
                17,
                true,
            ),
            (
                ["second-tool", "--help"].as_slice(),
                "second-tool",
                0,
                false,
            ),
            (
                ["second-tool", "--version"].as_slice(),
                "second-tool",
                0,
                false,
            ),
            (["second-tool", "check"].as_slice(), "second-tool", 2, false),
        ] {
            let (address, listener) = listener().await;
            let wire = tokio::spawn(async move {
                let (mut stream, request, length) = receive(&listener).await;
                assert_eq!(request, "POST /v1/traces HTTP/1.1\r\n");
                let mut body = vec![0; length];
                stream.read_exact(&mut body).await.unwrap();
                stream
                    .get_mut()
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
                assert!(!body.windows(13).any(|part| part == b"private-value"));
                crate::ExportTraceServiceRequest::decode(body.as_slice()).unwrap()
            });
            let result = run_tool(
                Config::new(
                    format!("http://{address}/v1/traces").parse().unwrap(),
                    "second-tool",
                    "1.0.0",
                ),
                args,
            )
            .await;
            if dispatched {
                assert_eq!(result.unwrap(), code);
            } else {
                let Err(ClapError::Parse(error)) = result else {
                    panic!("expected the original clap result");
                };
                assert_eq!(error.exit_code(), code);
                assert_eq!(error.use_stderr(), code != 0);
            }

            let exported = tokio::time::timeout(Duration::from_secs(5), wire)
                .await
                .unwrap()
                .unwrap();
            let spans = &exported.resource_spans[0].scope_spans[0].spans;
            assert_eq!(spans.len(), 1);
            let span = &spans[0];
            assert_eq!(span.name, name);
            assert!(span.attributes.iter().any(|attr| attr.key == "example.pre"));
            assert_eq!(
                span.attributes
                    .iter()
                    .any(|attr| attr.key == "example.post"),
                dispatched,
            );
            use opentelemetry_proto::tonic::common::v1::any_value::Value;
            let exit_code = span
                .attributes
                .iter()
                .find(|attr| attr.key == "process.exit.code")
                .unwrap()
                .value
                .as_ref()
                .and_then(|value| value.value.as_ref());
            assert_eq!(exit_code, Some(&Value::IntValue(i64::from(code))));
        }
    }

    #[tokio::test]
    async fn disabled_wrapper_leaves_outer_command_metadata_intact() {
        let telemetry = crate::Telemetry::new(Config::new(
            "http://127.0.0.1:4318/v1/traces".parse().unwrap(),
            "outer",
            "1.0.0",
        ))
        .unwrap();
        telemetry
            .run(
                crate::CommandMetadata::default(),
                ParentTrace::default(),
                run_clap(
                    None,
                    ParentTrace::default(),
                    clap::Command::new("nested").subcommand(clap::Command::new("check")),
                    ["nested", "check"],
                    || panic!("disabled enrichment"),
                    |_| async { Ok::<_, io::Error>(()) },
                    |_| panic!("disabled classification"),
                ),
                |_| Outcome::completed(0),
            )
            .await
            .unwrap();

        let spans = telemetry.buffer.drain();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "outer");
    }
}

#[cfg(feature = "reqwest-middleware")]
mod http_client {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use http::Extensions;
    use reqwest::{Client, Method, Request, Response, StatusCode};
    use reqwest_middleware::{ClientBuilder, Middleware, Next, Result};

    use crate::{
        CommandMetadata, Config, HttpMiddleware, KeyValue, Outcome, ParentTrace, SpanKind, Status,
        Telemetry,
    };

    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct ResponseMarker;

    struct AddResponseMarker;

    #[async_trait::async_trait]
    impl Middleware for AddResponseMarker {
        async fn handle(
            &self,
            request: Request,
            extensions: &mut Extensions,
            next: Next<'_>,
        ) -> Result<Response> {
            let mut response = next.run(request, extensions).await?;
            response.extensions_mut().insert(ResponseMarker);
            Ok(response)
        }
    }

    fn client() -> ClientBuilder {
        let _ = rustls::crypto::ring::default_provider().install_default();
        ClientBuilder::new(
            Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        )
    }

    fn telemetry() -> Telemetry {
        Telemetry::new(Config::new(
            "http://127.0.0.1:4318/v1/traces".parse().unwrap(),
            "tool",
            "1.0.0",
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn middleware_preserves_redirected_response_and_tracks_delayed_body() {
        let (address, listener) = listener().await;
        let body_delay = Duration::from_millis(125);
        let server = tokio::spawn(async move {
            let (mut outside, request, _) = receive(&listener).await;
            assert_eq!(request, "GET /outside HTTP/1.1\r\n");
            outside
                .get_mut()
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            drop(outside);

            let (mut redirect, request, _) = receive(&listener).await;
            assert_eq!(request, "PUT /known?value=private-query HTTP/1.1\r\n");
            redirect.get_mut().write_all(
                b"HTTP/1.1 307 Temporary Redirect\r\nLocation: /final?token=private-redirect\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ).await.unwrap();
            drop(redirect);

            let (mut body, request, _) = receive(&listener).await;
            assert_eq!(request, "PUT /final?token=private-redirect HTTP/1.1\r\n");
            body.get_mut().write_all(
                b"HTTP/1.1 201 Created\r\nContent-Length: 4\r\nX-Example: kept\r\nConnection: close\r\n\r\n",
            ).await.unwrap();
            tokio::time::sleep(body_delay).await;
            body.get_mut().write_all(b"data").await.unwrap();
        });

        let calls = Arc::new(AtomicUsize::new(0));
        let client = client()
            .with(HttpMiddleware::new({
                let calls = Arc::clone(&calls);
                move |request| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    if request.url().path() == "/known" {
                        vec![
                            KeyValue::new("url.template", "/known"),
                            KeyValue::new("example.region", "local"),
                        ]
                    } else {
                        Vec::new()
                    }
                }
            }))
            .with(AddResponseMarker)
            .build();

        client
            .get(format!("http://{address}/outside"))
            .send()
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        let telemetry = telemetry();
        telemetry
            .run(
                CommandMetadata::default(),
                ParentTrace::default(),
                async {
                    let response = client
                        .request(
                            Method::PUT,
                            format!("http://{address}/known?value=private-query"),
                        )
                        .send()
                        .await
                        .unwrap();
                    assert_eq!(calls.load(Ordering::Relaxed), 1);
                    assert_eq!(response.status(), StatusCode::CREATED);
                    assert_eq!(response.version(), reqwest::Version::HTTP_11);
                    assert_eq!(
                        response.url().as_str(),
                        format!("http://{address}/final?token=private-redirect"),
                    );
                    assert_eq!(response.headers()["x-example"], "kept");
                    assert_eq!(response.content_length(), Some(4));
                    assert_eq!(
                        response.extensions().get::<ResponseMarker>(),
                        Some(&ResponseMarker)
                    );
                    assert!(telemetry.buffer.0.lock().unwrap().is_empty());
                    assert_eq!(response.text().await.unwrap(), "data");
                },
                |_| Outcome::completed(0),
            )
            .await;
        server.await.unwrap();

        let spans = telemetry.buffer.drain();
        let [request, command] = spans.as_slice() else {
            panic!("expected one child and one command span");
        };
        assert_eq!(request.name, "PUT");
        assert_eq!(request.span_kind, SpanKind::Client);
        assert_eq!(request.parent_span_id, command.span_context.span_id());
        assert_eq!(request.status, Status::Unset);
        assert!(request.end_time.duration_since(request.start_time).unwrap() >= body_delay);
        for attribute in [
            KeyValue::new("http.request.method", "PUT"),
            KeyValue::new("server.address", "127.0.0.1"),
            KeyValue::new("server.port", i64::from(address.port())),
            KeyValue::new("http.response.status_code", 201),
            KeyValue::new("url.template", "/known"),
            KeyValue::new("example.region", "local"),
        ] {
            assert!(request.attributes.contains(&attribute));
        }
        assert!(!format!("{:?}", request.attributes).contains("private-"));
    }

    #[tokio::test]
    async fn middleware_finalizes_truncated_timed_out_and_dropped_bodies() {
        let telemetry = telemetry();
        let client = client().with(HttpMiddleware::new(|_| Vec::new())).build();
        telemetry.run(
            CommandMetadata::default(),
            ParentTrace::default(),
            async {
                for (index, path) in ["/truncated", "/timeout", "/dropped"].into_iter().enumerate() {
                    let (address, listener) = listener().await;
                    let (release, hold) = tokio::sync::oneshot::channel::<()>();
                    let server = tokio::spawn(async move {
                        let (mut stream, request, _) = receive(&listener).await;
                        assert_eq!(request, format!("GET {path} HTTP/1.1\r\n"));
                        stream.get_mut().write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\npartial",
                        ).await.unwrap();
                        if path != "/truncated" {
                            hold.await.unwrap();
                        }
                    });

                    let response = client.get(format!("http://{address}{path}"))
                        .timeout(Duration::from_millis(250)).send().await.unwrap();
                    assert_eq!(telemetry.buffer.0.lock().unwrap().len(), index);
                    if path == "/dropped" {
                        drop(response);
                    } else {
                        let error = response.text().await.unwrap_err();
                        assert!(error.is_decode());
                        assert_eq!(error.is_timeout(), path == "/timeout");
                    }
                    if path != "/truncated" {
                        release.send(()).unwrap();
                    }
                    server.await.unwrap();
                    assert_eq!(telemetry.buffer.0.lock().unwrap().len(), index + 1);
                }
            },
            |_| Outcome::completed(0),
        ).await;

        let spans = telemetry.buffer.drain();
        let [truncated, timed_out, dropped, _command] = spans.as_slice() else {
            panic!("expected three child spans and a command span");
        };
        for (span, error_type) in [(truncated, "http"), (timed_out, "timeout")] {
            assert_eq!(span.status, Status::error(""));
            assert!(span
                .attributes
                .contains(&KeyValue::new("error.type", error_type)));
        }
        assert_eq!(dropped.status, Status::Unset);
    }
}
