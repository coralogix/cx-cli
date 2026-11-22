#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::service_catalog::{
    run_archive_insight_types, run_data, run_entities, run_entity_data,
    run_entity_insight_drilldown, run_entity_insights, run_entity_types, run_insight_drilldown,
    run_insights, run_schema, InsightSelectionInput,
};
use coralogix_cli::config::OutputFormat;

const BASE: &str = "/v2/entities";

fn entity_types_body() -> serde_json::Value {
    json!({
        "entityTypes": [
            {
                "type": "ENTITY_TYPE_SERVICE",
                "id": "service",
                "displayName": "Service",
                "description": "APM services"
            }
        ]
    })
}

#[tokio::test]
async fn entity_types_returns_mappings_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(entity_types_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_entity_types(&targets, OutputFormat::Json)
        .await
        .expect("run_entity_types should succeed");
}

#[tokio::test]
async fn entity_types_merges_multiple_profiles() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    for server in [&server_a, &server_b] {
        Mock::given(method("GET"))
            .and(path(BASE))
            .respond_with(ResponseTemplate::new(200).set_body_json(entity_types_body()))
            .expect(1)
            .mount(server)
            .await;
    }

    let targets = vec![
        common::test_target("profile-a", &server_a.uri()),
        common::test_target("profile-b", &server_b.uri()),
    ];

    run_entity_types(&targets, OutputFormat::Json)
        .await
        .expect("run_entity_types should merge both profiles");
}

#[tokio::test]
async fn entity_types_tolerates_one_failing_profile() {
    let server_ok = MockServer::start().await;
    let server_err = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(200).set_body_json(entity_types_body()))
        .expect(1)
        .mount(&server_ok)
        .await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(json!({ "message": "backend exploded" })),
        )
        .expect(1)
        .mount(&server_err)
        .await;

    let targets = vec![
        common::test_target("profile-ok", &server_ok.uri()),
        common::test_target("profile-err", &server_err.uri()),
    ];

    run_entity_types(&targets, OutputFormat::Json)
        .await
        .expect("one failing profile should be non-fatal");
}

#[tokio::test]
async fn entity_types_errors_when_all_profiles_fail() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "message": "boom" })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let result = run_entity_types(&targets, OutputFormat::Json).await;
    assert!(result.is_err(), "all profiles failing should be an error");
}

#[tokio::test]
async fn all_output_formats_render_entity_types() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(BASE))
            .respond_with(ResponseTemplate::new(200).set_body_json(entity_types_body()))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];

        run_entity_types(&targets, format)
            .await
            .unwrap_or_else(|e| panic!("run_entity_types should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn schema_hits_the_normalized_entity_type_path() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE/metadata")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "entityId": "service",
            "displayName": "Service",
            "columns": [{ "id": "latency_p99", "displayName": "P99 Latency" }],
            "groupByLimit": 3
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_schema(&targets, "service", OutputFormat::Json)
        .await
        .expect("run_schema should hit the normalized path");
}

#[tokio::test]
async fn schema_accepts_hyphenated_short_entity_type() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/ENTITY_TYPE_K8S_POD/metadata")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "entityId": "k8s_pod" })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_schema(&targets, "k8s-pod", OutputFormat::Json)
        .await
        .expect("run_schema should normalize k8s-pod");
}

#[tokio::test]
async fn schema_rejects_unknown_entity_type_before_any_request() {
    let server = MockServer::start().await;
    // No mocks mounted: an invalid entity type must fail client-side without HTTP.

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_schema(&targets, "pod", OutputFormat::Json)
        .await
        .expect_err("unknown entity type should error");
    assert!(err.to_string().contains("unknown entity type 'pod'"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn entities_lists_known_entities() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "entities": [
                {
                    "name": "checkout",
                    "system": "kubernetes",
                    "lastSeen": "2026-07-01T00:00:00Z",
                    "environments": [{ "name": "prod", "lastSeen": "2026-07-01T00:00:00Z" }]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_entities(&targets, "service", OutputFormat::Json)
        .await
        .expect("run_entities should succeed");
}

#[tokio::test]
async fn entities_handles_empty_response() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "entities": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_entities(&targets, "service", OutputFormat::Text)
        .await
        .expect("empty entities response should succeed in text mode");
}

#[tokio::test]
async fn data_posts_the_expected_body_and_flattens_table_rows() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE/data")))
        .and(body_json(json!({
            "timeRange": { "start": 1000, "end": 2000 },
            "dataAggregationType": "DATA_AGGREGATION_TYPE_TABLE",
            "columns": [{ "columnId": "latency_p99" }],
            "groupBy": [],
            "filters": [{ "labelName": "environment", "labelValues": ["prod"] }],
            "limit": 5,
            "sortColumn": "latency_p99",
            "sortOrder": "SORT_ORDER_DESCENDING"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "table": {
                "rows": [
                    {
                        "identity": { "name": "checkout" },
                        "values": { "latency_p99": { "value": { "metric": 42.5 } } }
                    }
                ],
                "columns": [{ "id": "latency_p99" }]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_data(
        &targets,
        "service",
        "1970-01-01T00:16:40Z",
        "1970-01-01T00:33:20Z",
        &["latency_p99".to_string()],
        &[],
        &["environment=prod".to_string()],
        "table",
        Some(5),
        Some("latency_p99"),
        Some("desc"),
        OutputFormat::Json,
    )
    .await
    .expect("run_data should send the expected body and flatten the response");
}

#[tokio::test]
async fn data_rejects_table_controls_with_timeseries_aggregation_before_any_request() {
    let server = MockServer::start().await;
    // No mocks mounted: the combination must fail client-side without HTTP.

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_data(
        &targets,
        "service",
        "now-1h",
        "now",
        &["latency_p99".to_string()],
        &[],
        &[],
        "timeseries",
        Some(5),
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("limit with timeseries aggregation should error");
    assert!(err
        .to_string()
        .contains("only apply to --aggregation table"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn data_rejects_empty_columns_before_any_request() {
    let server = MockServer::start().await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_data(
        &targets,
        "service",
        "now-1h",
        "now",
        &[],
        &[],
        &[],
        "table",
        None,
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("empty --column list should error");
    assert!(err.to_string().contains("--column is required"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn data_handles_timeseries_aggregation() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE/data")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "timeseries": {
                "series": [{ "columnId": "latency_p99", "datapoints": [] }],
                "columns": [{ "id": "latency_p99" }],
                "totalSeriesCount": 1
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_data(
        &targets,
        "service",
        "now-1h",
        "now",
        &["latency_p99".to_string()],
        &[],
        &[],
        "timeseries",
        None,
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("run_data should handle timeseries responses");
}

#[tokio::test]
async fn data_errors_on_malformed_column_result() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE/data")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "table": {
                "rows": [
                    {
                        "identity": { "name": "checkout" },
                        "values": { "latency_p99": {} }
                    }
                ],
                "columns": []
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_data(
        &targets,
        "service",
        "now-1h",
        "now",
        &["latency_p99".to_string()],
        &[],
        &[],
        "table",
        None,
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("a malformed column result must surface as an error, not silent data loss");
    let chain = format!("{err:#}");
    assert!(chain.contains("expected exactly one of 'value' or 'error'"));
}

/// A malformed payload from one profile must not discard another profile's
/// good rows - formatting failures follow the same partial-failure semantics
/// as HTTP failures (report to stderr, render the survivors).
#[tokio::test]
async fn data_preserves_good_profile_when_another_profile_is_malformed() {
    let server_ok = MockServer::start().await;
    let server_malformed = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE/data")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "table": {
                "rows": [
                    {
                        "identity": { "name": "checkout" },
                        "values": { "latency_p99": { "value": { "metric": 42.5 } } }
                    }
                ],
                "columns": [{ "id": "latency_p99" }]
            }
        })))
        .expect(1)
        .mount(&server_ok)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("{BASE}/ENTITY_TYPE_SERVICE/data")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "table": {
                "rows": [
                    {
                        "identity": { "name": "checkout" },
                        "values": { "latency_p99": {} }
                    }
                ],
                "columns": []
            }
        })))
        .expect(1)
        .mount(&server_malformed)
        .await;

    let targets = vec![
        common::test_target("profile-ok", &server_ok.uri()),
        common::test_target("profile-malformed", &server_malformed.uri()),
    ];

    run_data(
        &targets,
        "service",
        "now-1h",
        "now",
        &["latency_p99".to_string()],
        &[],
        &[],
        "table",
        None,
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect("one profile's malformed payload must not discard another profile's good rows");
}

#[tokio::test]
async fn entity_data_posts_to_the_entity_scoped_path() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!(
            "{BASE}/ENTITY_TYPE_SERVICE/data/entity/checkout"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "table": {
                "rows": [
                    {
                        "identity": { "name": "checkout" },
                        "values": { "latency_p99": { "value": { "metric": 12.0 } } }
                    }
                ],
                "columns": []
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_entity_data(
        &targets,
        "service",
        "checkout",
        "now-1h",
        "now",
        &["latency_p99".to_string()],
        &[],
        &[],
        "table",
        OutputFormat::Json,
    )
    .await
    .expect("run_entity_data should succeed");
}

#[tokio::test]
async fn entity_data_percent_encodes_entity_id() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!(
            "{BASE}/ENTITY_TYPE_SERVICE/data/entity/checkout%2Fapi"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "table": { "rows": [], "columns": [] }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_entity_data(
        &targets,
        "service",
        "checkout/api",
        "now-1h",
        "now",
        &["latency_p99".to_string()],
        &[],
        &[],
        "table",
        OutputFormat::Json,
    )
    .await
    .expect("run_entity_data should percent-encode the entity id");
}

#[tokio::test]
async fn entity_data_rejects_empty_entity_id_before_any_request() {
    let server = MockServer::start().await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_entity_data(
        &targets,
        "service",
        "  ",
        "now-1h",
        "now",
        &["latency_p99".to_string()],
        &[],
        &[],
        "table",
        OutputFormat::Json,
    )
    .await
    .expect_err("blank entity id should error");
    assert!(err.to_string().contains("entity id must not be empty"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn api_error_body_surfaces_in_message() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(BASE))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "message": "invalid request"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_entity_types(&targets, OutputFormat::Json)
        .await
        .expect_err("400 from the only profile should error");
    let chain = format!("{err:#}");
    assert!(chain.contains("invalid request"));
    assert!(chain.contains("profile 'test-profile' failed"));
}

// ── Insights ──────────────────────────────────────────────────────────────────

const INSIGHTS: &str = "/v2/entities/insights";

const NO_SELECTION: InsightSelectionInput<'static> = InsightSelectionInput {
    types: &[],
    categories: &[],
    excluded_types: &[],
    excluded_categories: &[],
    archived: None,
};

fn entities_insights_body() -> serde_json::Value {
    json!({
        "perEntityType": [
            {
                "entityType": "ENTITY_TYPE_SERVICE",
                "insights": [
                    {
                        "type": "INSIGHT_TYPE_LATENCY_REGRESSION",
                        "category": "INSIGHT_CATEGORY_DEPLOYMENT",
                        "title": "2 services got slower after their last deployment",
                        "behaviorCoverage": {
                            "atLeastOne": { "insightCount": 2, "totalEntities": 10, "percentage": 20 }
                        },
                        "requestKind": { "kind": "REQUEST_KIND_SERVER" }
                    }
                ],
                "uncheckedInsights": [
                    {
                        "type": "INSIGHT_TYPE_NO_SLO",
                        "category": "INSIGHT_CATEGORY_ENABLEMENT",
                        "reason": "INSIGHT_UNCHECKED_REASON_SOURCE_UNAVAILABLE"
                    }
                ],
                "behaviorComparisonCoverage": { "eligibleChanges": 12, "compared": 10 }
            }
        ],
        "companyInsights": [
            {
                "type": "INSIGHT_TYPE_ENTITIES_LIMIT_NEAR",
                "category": "INSIGHT_CATEGORY_LIMITS",
                "title": "Close to the entities limit",
                "capUsage": { "used": 9500, "cap": 10000, "percentage": 95 }
            }
        ]
    })
}

#[tokio::test]
async fn insights_posts_the_selection_and_renders_every_format() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(INSIGHTS))
            .and(body_json(json!({
                "entityTypes": ["ENTITY_TYPE_SERVICE"],
                "filters": [{ "labelName": "environment", "labelValues": ["prod"] }],
                "breakdowns": ["INSIGHT_DIMENSION_ENVIRONMENT"],
                "types": [],
                "categories": ["INSIGHT_CATEGORY_DEPLOYMENT"],
                "excludedTypes": ["INSIGHT_TYPE_TRAFFIC_DROP"],
                "excludedCategories": [],
                "topValuesPerBreakdown": 5,
                "breakdownSort": "INSIGHT_BREAKDOWN_SORT_COUNT"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(entities_insights_body()))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];
        let categories = vec!["deployment".to_string()];
        let excluded_types = vec!["traffic-drop".to_string()];

        run_insights(
            &targets,
            &["service".to_string()],
            &["environment=prod".to_string()],
            &InsightSelectionInput {
                types: &[],
                categories: &categories,
                excluded_types: &excluded_types,
                excluded_categories: &[],
                archived: None,
            },
            &["environment".to_string()],
            Some(5),
            Some("count"),
            format,
        )
        .await
        .unwrap_or_else(|e| panic!("run_insights should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn insights_tolerates_one_failing_profile() {
    let server_ok = MockServer::start().await;
    let server_err = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(INSIGHTS))
        .respond_with(ResponseTemplate::new(200).set_body_json(entities_insights_body()))
        .expect(1)
        .mount(&server_ok)
        .await;
    Mock::given(method("POST"))
        .and(path(INSIGHTS))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "message": "boom" })))
        .expect(1)
        .mount(&server_err)
        .await;

    let targets = vec![
        common::test_target("profile-ok", &server_ok.uri()),
        common::test_target("profile-err", &server_err.uri()),
    ];

    run_insights(
        &targets,
        &["service".to_string()],
        &[],
        &NO_SELECTION,
        &[],
        None,
        None,
        OutputFormat::Text,
    )
    .await
    .expect("one failing profile should be non-fatal");
}

#[tokio::test]
async fn insights_rejects_unknown_category_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];
    let categories = vec!["capacity".to_string()];

    let err = run_insights(
        &targets,
        &["service".to_string()],
        &[],
        &InsightSelectionInput {
            types: &[],
            categories: &categories,
            excluded_types: &[],
            excluded_categories: &[],
            archived: None,
        },
        &[],
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("an unknown category should error");
    assert!(err.to_string().contains("unknown --category 'capacity'"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn entity_insights_posts_the_entity_identity() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{INSIGHTS}/entity")))
        .and(body_json(json!({
            "entity": { "entityType": "ENTITY_TYPE_DATABASE", "name": "orders", "system": "postgresql" },
            "environments": ["prod"],
            "types": [],
            "categories": [],
            "excludedTypes": [],
            "excludedCategories": []
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "insights": [
                {
                    "type": "INSIGHT_TYPE_NO_OWNER",
                    "category": "INSIGHT_CATEGORY_CONFIGURATION",
                    "title": "No owner",
                    "environments": ["prod"]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_entity_insights(
        &targets,
        "database",
        "orders",
        Some("postgresql"),
        &["prod".to_string()],
        &NO_SELECTION,
        None,
        None,
        OutputFormat::Text,
    )
    .await
    .expect("run_entity_insights should succeed");
}

fn archived_selection(archived: Option<&str>) -> InsightSelectionInput<'_> {
    InsightSelectionInput {
        archived,
        ..NO_SELECTION
    }
}

/// The body each `--archived` value sends: `exclude` and no flag send no
/// filter, which already hides archived types.
fn archive_filter_body(base: serde_json::Value, archive_filter: Option<&str>) -> serde_json::Value {
    let mut body = base;
    if let Some(archive_filter) = archive_filter {
        body["archiveFilter"] = json!(archive_filter);
    }
    body
}

const ARCHIVED_FLAG_CASES: &[(Option<&str>, Option<&str>)] = &[
    (None, None),
    (Some("exclude"), None),
    (Some("only"), Some("INSIGHT_ARCHIVE_FILTER_ARCHIVED_ONLY")),
    (Some("include"), Some("INSIGHT_ARCHIVE_FILTER_ALL")),
];

#[tokio::test]
async fn insights_sends_the_archive_filter_for_each_archived_value() {
    for &(archived, archive_filter) in ARCHIVED_FLAG_CASES {
        let server = MockServer::start().await;
        let expected = archive_filter_body(
            json!({
                "entityTypes": ["ENTITY_TYPE_SERVICE"],
                "filters": [],
                "breakdowns": [],
                "types": [],
                "categories": [],
                "excludedTypes": [],
                "excludedCategories": []
            }),
            archive_filter,
        );
        Mock::given(method("POST"))
            .and(path(INSIGHTS))
            .and(body_json(expected))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "archivedTypes": ["INSIGHT_TYPE_NO_SLO"]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];
        run_insights(
            &targets,
            &["service".to_string()],
            &[],
            &archived_selection(archived),
            &[],
            None,
            None,
            OutputFormat::Text,
        )
        .await
        .unwrap_or_else(|e| panic!("--archived {archived:?} should succeed: {e:#}"));
    }
}

#[tokio::test]
async fn entity_insights_sends_the_archive_filter_for_each_archived_value() {
    for &(archived, archive_filter) in ARCHIVED_FLAG_CASES {
        let server = MockServer::start().await;
        let expected = archive_filter_body(
            json!({
                "entity": { "entityType": "ENTITY_TYPE_SERVICE", "name": "checkout" },
                "environments": [],
                "types": [],
                "categories": [],
                "excludedTypes": [],
                "excludedCategories": []
            }),
            archive_filter,
        );
        Mock::given(method("POST"))
            .and(path(format!("{INSIGHTS}/entity")))
            .and(body_json(expected))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "archiveUnavailable": true
            })))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];
        run_entity_insights(
            &targets,
            "service",
            "checkout",
            None,
            &[],
            &archived_selection(archived),
            None,
            None,
            OutputFormat::Text,
        )
        .await
        .unwrap_or_else(|e| panic!("--archived {archived:?} should succeed: {e:#}"));
    }
}

#[tokio::test]
async fn insights_rejects_unknown_archived_value_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_entity_insights(
        &targets,
        "service",
        "checkout",
        None,
        &[],
        &archived_selection(Some("all")),
        None,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("an unknown --archived value should error");
    assert!(
        err.to_string().contains("unknown --archived 'all'"),
        "got: {err}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn insight_drilldown_posts_scope_sort_and_pagination() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{INSIGHTS}/drilldown")))
        .and(body_json(json!({
            "type": "INSIGHT_TYPE_LATENCY_REGRESSION",
            "entityType": "ENTITY_TYPE_SERVICE",
            "filters": [],
            "requestKind": { "kind": "REQUEST_KIND_SERVER" },
            "environmentMatch": "ENVIRONMENT_MATCH_EVERY",
            "sort": "INSIGHT_DRILLDOWN_SORT_SEVERITY",
            "sortOrder": "SORT_ORDER_DESCENDING",
            "nameSearch": "check",
            "pagination": { "pageIndex": 1, "pageSize": 20 }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "insight": { "type": "INSIGHT_TYPE_LATENCY_REGRESSION", "title": "Slower after deploy" },
            "entities": [
                {
                    "entity": { "entityType": "ENTITY_TYPE_SERVICE", "name": "checkout" },
                    "affectedEnvironmentCount": 1,
                    "environmentCount": 2,
                    "percentage": 50,
                    "affectedEnvironments": ["prod"]
                }
            ],
            "totalEntities": 21
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_insight_drilldown(
        &targets,
        "latency-regression",
        "service",
        &[],
        Some("request-kind=server"),
        Some("every"),
        Some("severity"),
        Some("desc"),
        Some(1),
        20,
        Some("check"),
        OutputFormat::Text,
    )
    .await
    .expect("run_insight_drilldown should succeed");
}

#[tokio::test]
async fn insight_drilldown_rejects_entity_only_scope_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_insight_drilldown(
        &targets,
        "operation-regression",
        "service",
        &[],
        Some("operation=GET /cart"),
        None,
        None,
        None,
        None,
        100,
        None,
        OutputFormat::Json,
    )
    .await
    .expect_err("operation scope belongs to entity-insight-drilldown");
    assert!(err.to_string().contains("invalid --scope"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn entity_insight_drilldown_posts_dimension_and_scope() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{INSIGHTS}/drilldown/entity")))
        .and(body_json(json!({
            "type": "INSIGHT_TYPE_DEPENDENCY_REGRESSION",
            "entity": { "entityType": "ENTITY_TYPE_SERVICE", "name": "checkout" },
            "dimension": "INSIGHT_DIMENSION_ENVIRONMENT",
            "environments": [],
            "dependency": { "database": { "dbSystem": "postgresql", "dbNamespace": "orders" } }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "insight": { "type": "INSIGHT_TYPE_DEPENDENCY_REGRESSION", "title": "orders got slower" },
            "dimension": "INSIGHT_DIMENSION_ENVIRONMENT",
            "values": [
                { "value": "prod", "isAffected": true, "affectedEnvironments": ["prod"] },
                { "value": "staging" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_entity_insight_drilldown(
        &targets,
        "dependency-regression",
        "service",
        "checkout",
        None,
        "environment",
        &[],
        Some("database=postgresql/orders"),
        OutputFormat::Text,
    )
    .await
    .expect("run_entity_insight_drilldown should succeed");
}

// ── Insight-type archive ──────────────────────────────────────────────────────

#[tokio::test]
async fn insight_types_archive_posts_the_types_and_renders_every_format() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(format!("{INSIGHTS}/types/archive")))
            .and(body_json(json!({
                "types": ["INSIGHT_TYPE_NO_SLO", "INSIGHT_TYPE_LATENCY_REGRESSION"]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "archivedTypes": [
                    "INSIGHT_TYPE_NO_OWNER",
                    "INSIGHT_TYPE_NO_SLO",
                    "INSIGHT_TYPE_LATENCY_REGRESSION"
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];
        run_archive_insight_types(
            &targets,
            &[
                "no-slo".to_string(),
                "INSIGHT_TYPE_LATENCY_REGRESSION".to_string(),
            ],
            true,
            format,
        )
        .await
        .unwrap_or_else(|e| panic!("archive should render {format:?}: {e:#}"));
    }
}

/// Proto JSON omits an empty list, so unarchiving the last type returns `{}`.
#[tokio::test]
async fn insight_types_unarchive_handles_the_empty_archived_set() {
    for format in [OutputFormat::Text, OutputFormat::Json] {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(format!("{INSIGHTS}/types/unarchive")))
            .and(body_json(json!({ "types": ["INSIGHT_TYPE_NO_SLO"] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];
        run_archive_insight_types(&targets, &["no-slo".to_string()], false, format)
            .await
            .unwrap_or_else(|e| panic!("unarchive should render {format:?}: {e:#}"));
    }
}

#[tokio::test]
async fn insight_types_archive_surfaces_a_permission_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("{INSIGHTS}/types/archive")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "code": "PERMISSION_DENIED",
            "message": "missing permission service-catalog:Update"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let err =
        run_archive_insight_types(&targets, &["no-slo".to_string()], true, OutputFormat::Text)
            .await
            .expect_err("a 403 should error");
    let chain = format!("{err:#}");
    assert!(chain.contains("service-catalog:Update"), "got: {chain}");
    assert!(
        chain.contains("profile 'test-profile' failed"),
        "got: {chain}"
    );
}

#[tokio::test]
async fn insight_types_archive_rejects_unknown_type_before_any_request() {
    let server = MockServer::start().await;
    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_archive_insight_types(
        &targets,
        &["no-such-type".to_string()],
        true,
        OutputFormat::Text,
    )
    .await
    .expect_err("an unknown type should error");
    assert!(
        err.to_string().contains("unknown --type 'no-such-type'"),
        "got: {err}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
