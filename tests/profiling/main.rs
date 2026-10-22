#[path = "../common/mod.rs"]
mod common;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::profiling::{
    run_filters, run_flame_graph, run_metadata, run_metrics, run_services, FlameGraphOptions,
};
use coralogix_cli::config::OutputFormat;

const SERVICES_PATH: &str = "/profiles/services/v1/overview-by-category";
const METADATA_PATH: &str = "/profiles/services/v1/metadata";
const FILTER_KEYS_PATH: &str = "/profiles/filters/v1/keys";
const FILTERS_PATH: &str = "/profiles/filters/v1";
const METRICS_PATH: &str = "/profiles/metrics/v1";
const FLAME_GRAPH_PATH: &str = "/profiles/flame-graph/v1";

fn cpu_services_body() -> Value {
    json!({
        "services": [{
            "serviceName": "checkout",
            "lastActive": "1792588680",
            "language": "go",
            "cpu": {
                "series": [{ "timestamp": "1792585242", "value": 1.5 }],
                "totalCpuSeconds": 120.5,
                "samplesPerSec": 10.0,
                "wallSeries": [],
                "totalWallSeconds": 0.0,
                "wallSamplesPerSecond": 0.0
            }
        }]
    })
}

fn metadata_body() -> Value {
    json!({
        "service": {
            "language": "go",
            "profileTypes": [
                { "profileType": "CPU", "hasData": true },
                { "profileType": "MEMORY_HEAP_BYTES", "hasData": false }
            ]
        }
    })
}

fn flame_graph_options() -> FlameGraphOptions {
    FlameGraphOptions {
        show_kernel: false,
        min_node_ratio: 0.01,
        max_depth: None,
        top_functions: 20,
    }
}

fn flame_graph_body(depth: usize) -> Value {
    let mut node = json!({
        "address": "",
        "functionLocations": [{ "functionFilenameIndex": 2, "functionNameIndex": 3, "classification": "FRAME_CLASSIFICATION_USER_CODE" }],
        "frameType": "FRAME_TYPE_GO",
        "totalValue": 100.0,
        "selfValue": 100.0,
        "children": []
    });
    for _ in 1..depth {
        node = json!({
            "address": "",
            "functionLocations": [{ "functionFilenameIndex": 2, "functionNameIndex": 1 }],
            "frameType": "FRAME_TYPE_GO",
            "totalValue": 100.0,
            "selfValue": 0.0,
            "children": [node]
        });
    }
    json!({
        "flamegraph": node,
        "stringTable": {
            "strings": STANDARD.encode("runmain.gohandle"),
            "offset": [0, 0, 3, 10],
            "length": [0, 3, 7, 6]
        },
        "warnMessages": [],
        "wasSeen": true
    })
}

#[tokio::test]
async fn services_always_sends_limit_and_category() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(SERVICES_PATH))
        .and(body_partial_json(json!({
            "category": "PROFILE_CATEGORY_CPU",
            "limit": 20,
            "offset": 0,
            "filters": [],
            "dateRange": { "timeUnit": "SECONDS" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(cpu_services_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_services(
        &targets,
        "cpu",
        "now-1h",
        "now",
        &[],
        20,
        0,
        false,
        OutputFormat::Json,
    )
    .await
    .expect("run_services should succeed");
}

#[tokio::test]
async fn services_sends_environment_filter_and_paging() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(SERVICES_PATH))
        .and(body_partial_json(json!({
            "category": "PROFILE_CATEGORY_MEMORY",
            "limit": 5,
            "offset": 10,
            "filters": [{
                "key": "deployment.environment.name",
                "value": ["prod", "staging"],
                "predicate": "EQUAL"
            }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "services": [{
                "serviceName": "checkout",
                "lastActive": "1",
                "memory": { "totalMemoryAllocationsBytes": "4096", "totalHeapBytes": "1024" }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_services(
        &targets,
        "memory",
        "now-1h",
        "now",
        &["prod".to_string(), "staging".to_string()],
        5,
        10,
        false,
        OutputFormat::Text,
    )
    .await
    .expect("run_services should succeed");
}

#[tokio::test]
async fn services_merges_multiple_profiles_in_every_format() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server_a = MockServer::start().await;
        let server_b = MockServer::start().await;

        for server in [&server_a, &server_b] {
            Mock::given(method("POST"))
                .and(path(SERVICES_PATH))
                .respond_with(ResponseTemplate::new(200).set_body_json(cpu_services_body()))
                .expect(1)
                .mount(server)
                .await;
        }

        let targets = vec![
            common::test_target("profile-a", &server_a.uri()),
            common::test_target("profile-b", &server_b.uri()),
        ];

        run_services(&targets, "cpu", "now-1h", "now", &[], 20, 0, true, format)
            .await
            .unwrap_or_else(|e| panic!("run_services should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn services_tolerates_one_failing_profile() {
    let server_ok = MockServer::start().await;
    let server_err = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(SERVICES_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(cpu_services_body()))
        .expect(1)
        .mount(&server_ok)
        .await;

    Mock::given(method("POST"))
        .and(path(SERVICES_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "code": "INTERNAL",
            "message": "backend exploded"
        })))
        .expect(1)
        .mount(&server_err)
        .await;

    let targets = vec![
        common::test_target("profile-ok", &server_ok.uri()),
        common::test_target("profile-err", &server_err.uri()),
    ];

    run_services(
        &targets,
        "cpu",
        "now-1h",
        "now",
        &[],
        20,
        0,
        false,
        OutputFormat::Json,
    )
    .await
    .expect("one failing profile should be non-fatal");
}

#[tokio::test]
async fn services_surfaces_api_error_message() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(SERVICES_PATH))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": "INVALID_ARGUMENT",
            "message": "limit must be > 0"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_services(
        &targets,
        "cpu",
        "now-1h",
        "now",
        &[],
        20,
        0,
        false,
        OutputFormat::Json,
    )
    .await
    .expect_err("an API error should fail the command");
    assert!(
        format!("{err:#}").contains("limit must be > 0"),
        "got: {err:#}"
    );
}

#[tokio::test]
async fn services_errors_on_mismatched_category_data() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(SERVICES_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(cpu_services_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_services(
        &targets,
        "memory",
        "now-1h",
        "now",
        &[],
        20,
        0,
        false,
        OutputFormat::Json,
    )
    .await
    .expect_err("cpu data for a memory request should be an error");
    assert!(format!("{err:#}").contains("malformed profiling services response"));
}

#[tokio::test]
async fn services_rejects_invalid_arguments_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    for (category, limit, environments) in [
        ("disk", 20, vec![]),
        ("cpu", 0, vec![]),
        ("cpu", 101, vec![]),
        ("cpu", 20, vec!["it's".to_string()]),
    ] {
        let result = run_services(
            &targets,
            category,
            "now-1h",
            "now",
            &environments,
            limit,
            0,
            false,
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_err(), "{category}/{limit} should be rejected");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn metadata_posts_service_name_and_renders_every_format() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(METADATA_PATH))
            .and(body_partial_json(json!({ "serviceName": "checkout" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(metadata_body()))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];

        run_metadata(&targets, " checkout ", "now-1h", "now", format)
            .await
            .unwrap_or_else(|e| panic!("run_metadata should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn metadata_errors_on_missing_profile_types() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(METADATA_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "service": {} })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_metadata(&targets, "checkout", "now-1h", "now", OutputFormat::Json)
        .await
        .expect_err("empty metadata should be an error");
    assert!(format!("{err:#}").contains("missing service profile types"));
}

#[tokio::test]
async fn metadata_rejects_inverted_time_range_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_metadata(&targets, "checkout", "now", "now-1h", OutputFormat::Json)
        .await
        .expect_err("end before start should be rejected");
    assert!(err.to_string().contains("must be strictly after"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn filters_calls_keys_and_values_and_merges_them() {
    for format in [OutputFormat::Text, OutputFormat::Json] {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(FILTER_KEYS_PATH))
            .and(body_partial_json(json!({
                "serviceName": "checkout",
                "profileType": "MEMORY_HEAP_BYTES"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "filterKeys": ["k8s.pod.name", "host.name"]
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path(FILTERS_PATH))
            .and(body_partial_json(json!({
                "serviceName": "checkout",
                "profileType": "MEMORY_HEAP_BYTES",
                "filters": [{
                    "key": "deployment.environment.name",
                    "value": ["prod"],
                    "predicate": "NOT_EQUAL"
                }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "filters": { "k8s.pod.name": { "values": ["pod-b", "pod-a"] } }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];

        run_filters(
            &targets,
            "checkout",
            "memory-heap-bytes",
            "now-1h",
            "now",
            &["deployment.environment.name!=prod".to_string()],
            50,
            format,
        )
        .await
        .unwrap_or_else(|e| panic!("run_filters should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn filters_rejects_off_cpu_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_filters(
        &targets,
        "checkout",
        "OFF_CPU",
        "now-1h",
        "now",
        &[],
        50,
        OutputFormat::Json,
    )
    .await
    .expect_err("OFF_CPU should be rejected");
    assert!(err.to_string().contains("unknown profile type"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn metrics_sends_max_buckets_and_sum_by() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(METRICS_PATH))
            .and(body_partial_json(json!({
                "serviceName": "checkout",
                "profileType": "CPU",
                "sumBy": "pod",
                "maxBuckets": 60,
                "filters": [{ "key": "host.name", "value": [], "predicate": "EXISTS" }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "series": [
                    { "groupByValue": "pod-a", "points": [{ "timestamp": "10", "avgValue": 1.0, "totalValue": 60.0 }] },
                    { "groupByValue": "pod-b", "points": [{ "timestamp": "10", "avgValue": 2.0, "totalValue": 120.0 }] }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];

        run_metrics(
            &targets,
            "checkout",
            "cpu",
            "now-1h",
            "now",
            &["host.name:exists".to_string()],
            "pod",
            60,
            1,
            format,
        )
        .await
        .unwrap_or_else(|e| panic!("run_metrics should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn metrics_never_sends_bucket_size_or_top_k() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(METRICS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "series": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_metrics(
        &targets,
        "checkout",
        "cpu",
        "now-1h",
        "now",
        &[],
        "service",
        60,
        10,
        OutputFormat::Json,
    )
    .await
    .expect("run_metrics should succeed");

    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(body.get("bucketSizeMs").is_none());
    assert!(body.get("topK").is_none());
    assert_eq!(body["maxBuckets"], 60);
}

#[tokio::test]
async fn metrics_rejects_invalid_filter_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_metrics(
        &targets,
        "checkout",
        "cpu",
        "now-1h",
        "now",
        &["k8s.pod.name".to_string()],
        "service",
        60,
        10,
        OutputFormat::Json,
    )
    .await
    .expect_err("a filter without a predicate should be rejected");
    assert!(err.to_string().contains("invalid --filter"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn flame_graph_decodes_and_renders_every_format() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(FLAME_GRAPH_PATH))
            .and(body_partial_json(json!({
                "serviceName": "checkout",
                "profileType": "CPU",
                "showKernel": false,
                "filters": []
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(flame_graph_body(3)))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];

        run_flame_graph(
            &targets,
            "checkout",
            "cpu",
            "now-1h",
            "now",
            &[],
            flame_graph_options(),
            format,
        )
        .await
        .unwrap_or_else(|e| panic!("run_flame_graph should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn flame_graph_parses_trees_deeper_than_the_json_nesting_limit() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(FLAME_GRAPH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(flame_graph_body(200)))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_flame_graph(
        &targets,
        "checkout",
        "cpu",
        "now-1h",
        "now",
        &[],
        flame_graph_options(),
        OutputFormat::Json,
    )
    .await
    .expect("a 200-deep flame graph should parse");
}

#[tokio::test]
async fn flame_graph_rejects_trees_deeper_than_the_supported_depth() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(FLAME_GRAPH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(flame_graph_body(600)))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_flame_graph(
        &targets,
        "checkout",
        "cpu",
        "now-1h",
        "now",
        &[],
        flame_graph_options(),
        OutputFormat::Json,
    )
    .await
    .expect_err("a flame graph deeper than the supported depth should be an error");
    assert!(format!("{err:#}").contains("more than the supported 1000"));
}

#[tokio::test]
async fn flame_graph_errors_on_out_of_range_string_index() {
    let server = MockServer::start().await;

    let mut body = flame_graph_body(2);
    body["stringTable"]["offset"] = json!([0]);
    body["stringTable"]["length"] = json!([0]);

    Mock::given(method("POST"))
        .and(path(FLAME_GRAPH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_flame_graph(
        &targets,
        "checkout",
        "cpu",
        "now-1h",
        "now",
        &[],
        flame_graph_options(),
        OutputFormat::Json,
    )
    .await
    .expect_err("an out-of-range string index should be an error");
    assert!(format!("{err:#}").contains("out of range"));
}

#[tokio::test]
async fn flame_graph_rejects_min_node_ratio_below_server_prune_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let mut options = flame_graph_options();
    options.min_node_ratio = 0.0001;

    let err = run_flame_graph(
        &targets,
        "checkout",
        "cpu",
        "now-1h",
        "now",
        &[],
        options,
        OutputFormat::Json,
    )
    .await
    .expect_err("a ratio below the server prune ratio should be rejected");
    assert!(err.to_string().contains("--min-node-ratio"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
