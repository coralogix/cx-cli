use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opentelemetry_proto::tonic::{
    common::v1::{any_value::Value as OtlpValue, AnyValue, KeyValue as OtlpKeyValue},
    trace::v1::{ResourceSpans, ScopeSpans, Span as OtlpSpan},
};
use prost::Message;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::*;
use crate::limits::{MAX_ARRAY_ITEMS, MAX_BATCH_BYTES, MAX_KEY_BYTES};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestQueue {
    dir: PathBuf,
    spool: Spool,
}

impl TestQueue {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        let counter = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("cli-otel-queue-{pid}-{timestamp}-{counter}"));
        let spool = Spool::new(dir.clone());
        Self { dir, spool }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.otlp"))
    }

    fn write(&self, id: &str, payload: &[u8]) {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(self.path(id))
            .unwrap()
            .write_all(payload)
            .unwrap();
    }
}

impl Drop for TestQueue {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn example_request(sequence: u64) -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            scope_spans: vec![ScopeSpans {
                spans: vec![example_span(sequence)],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

fn example_span(sequence: u64) -> OtlpSpan {
    OtlpSpan {
        name: "tool example".into(),
        trace_id: vec![1; 16],
        span_id: sequence.to_be_bytes().to_vec(),
        parent_span_id: vec![2; 8],
        start_time_unix_nano: 1,
        end_time_unix_nano: 2,
        ..Default::default()
    }
}

fn sequence(payload: &[u8]) -> u64 {
    let request = ExportTraceServiceRequest::decode(payload).unwrap();
    let span = &request.resource_spans[0].scope_spans[0].spans[0];
    u64::from_be_bytes(span.span_id.as_slice().try_into().unwrap())
}

#[test]
fn sender_lock_is_nonblocking_and_independent_of_enqueue() {
    let TestQueue { spool, .. } = &TestQueue::new();
    assert!(spool.pending().unwrap().is_empty());

    let sender_lock = spool.try_lock().unwrap();
    assert_eq!(
        spool.try_lock().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    std::thread::scope(|scope| {
        for index in 1..=4 {
            scope.spawn(move || {
                spool
                    .enqueue(&example_request(index).encode_to_vec())
                    .unwrap()
            });
        }
    });
    let pending = spool.pending().unwrap();
    assert_eq!(pending.len(), 4);
    assert_eq!(
        pending
            .iter()
            .map(|batch| &batch.id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );

    spool.remove(&pending[0].id).unwrap();
    spool.remove(&pending[0].id).unwrap();
    assert_eq!(spool.pending().unwrap().len(), 3);
    drop(sender_lock);
    assert!(spool.try_lock().is_ok());
}

#[test]
fn queue_keeps_newest_128_batches_and_expires_old_records() {
    let queue = TestQueue::new();
    let spool = &queue.spool;
    for index in 1..=132 {
        spool
            .enqueue(&example_request(index).encode_to_vec())
            .unwrap();
    }
    let pending = spool.pending().unwrap();
    assert_eq!(pending.len(), 128);
    assert_eq!(sequence(&pending[0].payload), 5);
    assert_eq!(sequence(&pending[127].payload), 132);

    let oldest = queue.path(&pending[0].id);
    File::open(&oldest)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60))
        .unwrap();
    assert_eq!(spool.pending().unwrap().len(), 127);
    assert!(!oldest.exists());
}

#[test]
fn queue_keeps_at_most_eight_mib_of_valid_batches() {
    let queue = TestQueue::new();
    let mut request = example_request(1);
    let spans = &mut request.resource_spans[0].scope_spans[0].spans;
    for index in 2..=26 {
        spans.push(example_span(index));
    }
    for span in spans {
        span.attributes = (0..30)
            .map(|index| OtlpKeyValue {
                key: format!("example.field{index}"),
                value: Some(AnyValue {
                    value: Some(OtlpValue::StringValue("a".repeat(540))),
                }),
                key_strindex: 0,
            })
            .collect();
    }
    let body = request.encode_to_vec();
    assert!((400 * 1024..MAX_BATCH_BYTES).contains(&body.len()));
    for index in 1..=24 {
        request.resource_spans[0].scope_spans[0].spans[0].span_id =
            (100_u64 + index).to_be_bytes().to_vec();
        queue.spool.enqueue(&request.encode_to_vec()).unwrap();
    }

    let pending = queue.spool.pending().unwrap();
    let bytes: usize = pending.iter().map(|batch| batch.payload.len()).sum();
    assert!(pending.len() < 24);
    assert!(bytes <= 8 * 1024 * 1024);
    assert!(bytes + body.len() > 8 * 1024 * 1024);
    assert_eq!(sequence(&pending.last().unwrap().payload), 124);
    let files_on_disk = fs::read_dir(&queue.dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "otlp"))
        .count();
    assert_eq!(files_on_disk, pending.len());
}

#[test]
fn malformed_and_untrusted_queue_data_are_removed() {
    let queue = TestQueue::new();
    let spool = &queue.spool;
    let original = example_request(1).encode_to_vec();
    let mut with_unknown_fields = original.clone();
    let secret = b"unknown-field-credential";
    // Unknown field 99, wire type length-delimited.
    with_unknown_fields.extend_from_slice(&[0x9a, 0x06, secret.len() as u8]);
    with_unknown_fields.extend_from_slice(secret);
    spool.enqueue(&with_unknown_fields).unwrap();
    let first = spool.pending().unwrap();
    assert_eq!(first[0].payload, original);
    assert_eq!(fs::read(queue.path(&first[0].id)).unwrap(), original);

    for (index, invalid) in [
        b"unparseable".to_vec(),
        vec![0; MAX_BATCH_BYTES + 1],
        example_request(0).encode_to_vec(),
        {
            let mut request = example_request(1);
            request.resource_spans[0].scope_spans[0].spans[0].parent_span_id = vec![6; 7];
            request.encode_to_vec()
        },
        {
            let mut request = example_request(1);
            request.resource_spans[0].scope_spans[0].spans[0].status =
                Some(opentelemetry_proto::tonic::trace::v1::Status {
                    message: "private-stored-error".into(),
                    code: 2,
                });
            request.encode_to_vec()
        },
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            spool.enqueue(&invalid).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        queue.write(&format!("{index:040x}"), &invalid);
    }

    let abandoned = queue.dir.join(format!("{:040x}.tmp", 9));

    fs::write(&abandoned, b"half-written bytes").unwrap();
    File::open(&abandoned)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(90))
        .unwrap();
    let pending = spool.pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].payload, original);
    assert!(!abandoned.exists());
    for index in 0..5 {
        assert!(!queue.path(&format!("{index:040x}")).exists());
    }
    assert_eq!(
        spool.remove("../sender.lock").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[cfg(unix)]
#[test]
fn queue_files_are_private_and_symlinks_are_never_replayed() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let queue = TestQueue::new();
    let payload = example_request(1).encode_to_vec();
    queue.spool.enqueue(&payload).unwrap();
    let lock = queue.spool.try_lock().unwrap();
    assert_eq!(lock.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(&queue.dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let pending = queue.spool.pending().unwrap();
    let record = queue.path(&pending[0].id);
    assert_eq!(
        fs::metadata(&record).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let link = queue.path(&format!("{:040x}", 1));
    symlink(&record, &link).unwrap();
    let broad_id = format!("{:040x}", 2);
    queue.write(&broad_id, &payload);
    let broad = queue.path(&broad_id);
    fs::set_permissions(&broad, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(queue.spool.pending().unwrap().len(), 1);
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(!broad.exists());
    assert_eq!(fs::read(&record).unwrap(), payload);
}

#[tokio::test]
async fn inline_and_reopened_queue_export_bounded_private_otlp() {
    let queue = TestQueue::new();
    for spooled in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut config = Config::new(
            format!("http://{address}/v1/traces").parse().unwrap(),
            "tool",
            "1.2.3",
        );
        config.headers.insert(
            header::AUTHORIZATION,
            "Bearer private-header".parse().unwrap(),
        );
        config.resource_attributes = vec![
            KeyValue::new("example.resource", "safe"),
            KeyValue::new("example.large", "private-resource".repeat(MAX_VALUE_BYTES)),
        ];
        if spooled {
            config.delivery = Delivery::Spool(queue.spool.clone());
        }

        let capture = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(stream.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            stream.read_exact(&mut body).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            body
        };

        let run = async {
            let telemetry = Telemetry::new(config).unwrap();
            let mut paths = (0..MAX_COMMAND_DEPTH)
                .map(|_| "check".to_owned())
                .collect::<Vec<_>>();
            paths.push("private-depth".into());
            let mut flags = vec![
                "--value=private-option".into(),
                format!("--{}", "private-long-flag".repeat(10)),
            ];
            flags.extend((0..MAX_FLAGS).map(|index| format!("--option{index}")));
            flags.push("--private-extra-flag".into());
            telemetry
                .run(
                    CommandMetadata {
                        command_path: paths,
                        flags,
                    },
                    ParentTrace {
                        traceparent: Some(
                            "00-11111111111111111111111111111111-2222222222222222-01",
                        ),
                        tracestate: Some("vendor=sample"),
                    },
                    async {
                        record_attribute(KeyValue::new("example.valid", true));
                        record_attribute(KeyValue::new(
                            "invalid.key".repeat(MAX_KEY_BYTES),
                            "private-key",
                        ));
                        record_attribute(KeyValue::new(
                            "example.string",
                            "private-span-value".repeat(MAX_VALUE_BYTES),
                        ));
                        record_attribute(KeyValue::new(
                            "example.array",
                            Value::Array(Array::String(vec![
                                "private-array".into();
                                MAX_ARRAY_ITEMS + 1
                            ])),
                        ));
                        record_attribute(KeyValue::new("example.nonfinite", f64::NAN));
                        for index in 0..limits::MAX_ATTRIBUTES * 2 {
                            record_attribute(KeyValue::new(
                                format!("example.extension{index}"),
                                "x".repeat(500),
                            ));
                        }
                        let mut child =
                            start_child_span("GET", SpanKind::Client, Vec::new).unwrap();
                        child.set_status(Status::error("private-raw-error"));
                        child.end();
                    },
                    |_| Outcome {
                        error_type: Some("private outcome message"),
                        ..Outcome::completed(17)
                    },
                )
                .await;
            telemetry.shutdown().await;
        };

        let body = if spooled {
            run.await;
            assert!(tokio::time::timeout(Duration::from_millis(50), capture)
                .await
                .is_err());
            let reopened = Spool::new(queue.dir.clone());
            let pending = reopened.pending().unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(
                pending[0].payload,
                fs::read(queue.path(&pending[0].id)).unwrap()
            );
            assert_eq!(pending[0].payload, reopened.pending().unwrap()[0].payload);
            pending.into_iter().next().unwrap().payload
        } else {
            let ((), body) =
                tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(run, capture) })
                    .await
                    .unwrap();
            body
        };
        assert!(!body
            .windows(b"private".len())
            .any(|part| part == b"private"));
        let request = ExportTraceServiceRequest::decode(body.as_slice()).unwrap();
        let resource_spans = &request.resource_spans[0];
        let resource = resource_spans.resource.as_ref().unwrap();
        assert!(resource.attributes.iter().any(|attribute| {
            attribute.key == "service.version"
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    == Some(&OtlpValue::StringValue("1.2.3".into()))
        }));
        let [child, root] = resource_spans.scope_spans[0].spans.as_slice() else {
            panic!("expected a child and an execution span");
        };
        assert_eq!(root.trace_id, vec![0x11; 16]);
        assert_eq!(root.parent_span_id, vec![0x22; 8]);
        assert_eq!(root.trace_state, "vendor=sample");
        assert_eq!(
            root.name,
            format!("tool{}", " check".repeat(MAX_COMMAND_DEPTH))
        );
        assert_eq!(child.parent_span_id, root.span_id);
        assert_eq!(child.trace_id, root.trace_id);
        assert!(child.start_time_unix_nano >= root.start_time_unix_nano);
        assert!(child.end_time_unix_nano <= root.end_time_unix_nano);
        assert!(root.attributes.iter().any(|attribute| {
            attribute.key == "error.type"
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    == Some(&OtlpValue::StringValue("error".into()))
        }));
        assert!(root
            .attributes
            .iter()
            .any(|attribute| attribute.key == "example.valid"));
        assert!(!root
            .attributes
            .iter()
            .any(|attribute| attribute.key == "example.nonfinite"));
        assert!(root.attributes.iter().any(|attribute| {
            attribute.key == "process.exit.code"
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    == Some(&OtlpValue::IntValue(17))
        }));
        for key in [
            "process.executable.name",
            "process.pid",
            "cli.termination.reason",
        ] {
            assert!(root.attributes.iter().any(|attribute| attribute.key == key));
        }
        assert_eq!(root.status.as_ref().unwrap().code, 2);
        assert!(root
            .attributes
            .iter()
            .all(|attr| !matches!(attr.key.as_str(), "cli.command" | "cli.flags")));
        let args = root
            .attributes
            .iter()
            .find(|attribute| attribute.key == "process.command_args")
            .unwrap();
        let Some(OtlpValue::ArrayValue(args)) = &args.value.as_ref().unwrap().value else {
            panic!("expected sanitized command arguments");
        };
        let mut expected = vec!["tool".to_owned()];
        expected.extend(std::iter::repeat_n("check".to_owned(), MAX_COMMAND_DEPTH));
        expected
            .extend((0..MAX_ARRAY_ITEMS - expected.len()).map(|index| format!("--option{index}")));
        let args: Vec<_> = args
            .values
            .iter()
            .map(|item| match &item.value {
                Some(OtlpValue::StringValue(value)) => value.as_str(),
                _ => panic!("expected a string command argument"),
            })
            .collect();
        assert_eq!(args, expected);
    }
}

#[tokio::test]
async fn oversized_export_drops_old_spans_and_keeps_execution_span() {
    let queue = TestQueue::new();
    let mut config = Config::new(
        "http://127.0.0.1:1/v1/traces".parse().unwrap(),
        "tool",
        "1.0.0",
    );
    config.delivery = Delivery::Spool(queue.spool.clone());
    let telemetry = Telemetry::new(config).unwrap();
    telemetry
        .run(
            CommandMetadata::default(),
            ParentTrace::default(),
            async {
                for _ in 0..MAX_SPANS {
                    let mut child = start_child_span("GET", SpanKind::Client, || {
                        (0..32)
                            .map(|index| {
                                KeyValue::new(format!("example.attribute{index}"), "a".repeat(500))
                            })
                            .collect()
                    })
                    .unwrap();
                    child.end();
                }
            },
            |_| Outcome::completed(0),
        )
        .await;
    telemetry.shutdown().await;
    let pending = queue.spool.pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].payload.len() <= MAX_BATCH_BYTES);
    let request = ExportTraceServiceRequest::decode(pending[0].payload.as_slice()).unwrap();
    let spans = &request.resource_spans[0].scope_spans[0].spans;
    assert!((2..MAX_SPANS).contains(&spans.len()));
    let root = spans.last().unwrap();
    assert_eq!(root.name, "tool");
    assert!(spans[..spans.len() - 1]
        .iter()
        .all(|span| span.parent_span_id == root.span_id));
}

#[cfg(feature = "reqwest-middleware")]
#[tokio::test]
async fn http_completion_fields_survive_attribute_limit() {
    let queue = TestQueue::new();
    let mut config = Config::new(
        "http://127.0.0.1:1/v1/traces".parse().unwrap(),
        "tool",
        "1.0.0",
    );
    config.delivery = Delivery::Spool(queue.spool.clone());
    let telemetry = Telemetry::new(config).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        loop {
            let mut line = String::new();
            assert_ne!(stream.read_line(&mut line).await.unwrap(), 0);
            if line == "\r\n" {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    });

    let client =
        reqwest_middleware::ClientBuilder::new(inline_client(Duration::from_secs(2)).unwrap())
            .with(
                HttpMiddleware::new(|_| {
                    let mut attributes = vec![
                        KeyValue::new("error.type", "private-override"),
                        KeyValue::new("http.response.status_code", 200),
                    ];
                    attributes.extend((0..limits::MAX_ATTRIBUTES * 2).map(|index| {
                        KeyValue::new(format!("example.extension{index}"), "x".repeat(500))
                    }));
                    attributes
                })
                .with_server_address(false),
            )
            .build();

    telemetry
        .run(
            CommandMetadata::default(),
            ParentTrace::default(),
            async {
                let result = client
                    .get(format!("http://{address}/"))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(result.status(), 503);
            },
            |_| Outcome::completed(17),
        )
        .await;
    server.await.unwrap();
    telemetry.shutdown().await;
    let pending = queue.spool.pending().unwrap();
    let request = ExportTraceServiceRequest::decode(pending[0].payload.as_slice()).unwrap();
    let child = &request.resource_spans[0].scope_spans[0].spans[0];
    assert_eq!(child.name, "GET");
    assert_eq!(child.status.as_ref().unwrap().code, 2);
    let attributes = child
        .attributes
        .iter()
        .map(|attribute| {
            (
                attribute.key.as_str(),
                attribute.value.as_ref().unwrap().value.as_ref().unwrap(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        attributes.get("http.response.status_code"),
        Some(&&OtlpValue::IntValue(503))
    );
    assert_eq!(
        attributes.get("error.type"),
        Some(&&OtlpValue::StringValue("503".to_owned()))
    );
    assert!(!attributes.contains_key("server.address"));
    assert!(!attributes.contains_key("server.port"));
}
