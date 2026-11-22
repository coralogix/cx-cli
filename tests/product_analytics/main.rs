#[path = "../common/mod.rs"]
mod common;

use serde_json::{json, Value};
use wiremock::matchers::{body_json, body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::product_analytics::api::{
    EventSource, FunnelOutcome, PathwayDirection, QueryInput, QueryKind, SavedObjectKind,
};
use coralogix_cli::commands::product_analytics::{
    run_applications_get, run_applications_list, run_events_get, run_list_query, run_query,
    run_saved_delete, run_saved_get, run_saved_list, run_saved_write, run_settings_get,
    run_settings_update, PageArgs,
};
use coralogix_cli::config::OutputFormat;

const BASE: &str = "/mgmt/openapi/5/product-analytics";
const FIXED_RANGE: (i64, i64) = (1_763_251_200_000, 1_763_337_600_000);

fn fixed_range_json() -> Value {
    json!({"fixed": {"fromTimestampMs": FIXED_RANGE.0, "toTimestampMs": FIXED_RANGE.1}})
}

fn saved_funnel() -> Value {
    json!({
        "id": "42",
        "appName": "web-shop",
        "name": "Checkout funnel",
        "definition": {
            "mode": "FUNNEL_MODE_USERS",
            "steps": [
                {"stepId": "s1", "conditions": [{"view": {"name": {"values": ["/cart"], "operator": "STRING_CONDITION_OPERATOR_EQUAL"}}}]}
            ]
        },
        "filters": [],
        "timeRange": {"relativeMinutes": "1440"},
        "owner": "dana@example.com",
        "createdAt": "2026-11-16T10:00:00Z",
        "updatedAt": "2026-11-16T10:00:00Z",
        "version": 1
    })
}

fn write_json(name: &str, body: &Value) -> std::path::PathBuf {
    let file = std::env::temp_dir().join(format!("cx_pa_test_{}_{name}.json", std::process::id()));
    std::fs::write(&file, serde_json::to_vec(body).unwrap()).unwrap();
    file
}

#[tokio::test]
async fn applications_list_sends_app_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/applications/v1")))
        .and(query_param("app_names", "web-shop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "applications": [{"appName": "web-shop", "sessions": "18420", "crashFreeSessions": "18391", "users": "6210", "crashFreeUsers": "6198"}]
        })))
        .expect(3)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        run_applications_list(&targets, &["web-shop".to_string()], output)
            .await
            .expect("applications list should succeed");
    }
}

#[tokio::test]
async fn applications_list_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/applications/v1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_applications_list(&targets, &[], OutputFormat::Text)
        .await
        .expect("empty applications list should succeed");
}

#[tokio::test]
async fn applications_get_encodes_the_app_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/applications/v1/web%20shop")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "appName": "web shop", "sessions": "1", "crashFreeSessions": "1", "users": "1", "crashFreeUsers": "1"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_applications_get(&targets, "web shop", OutputFormat::Text)
        .await
        .expect("applications get should succeed");
}

#[tokio::test]
async fn disabled_team_error_carries_the_server_message() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/funnels/v1")))
        .respond_with(ResponseTemplate::new(412).set_body_json(json!({
            "code": 9,
            "message": "Product Analytics is not enabled for this team. A team admin can enable it in Coralogix.",
            "details": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = run_saved_list(
        &targets,
        SavedObjectKind::Funnel,
        &[],
        &PageArgs::default(),
        OutputFormat::Json,
    )
    .await
    .expect_err("412 must fail");
    let message = format!("{error:#}");
    assert!(message.contains("412"), "{message}");
    assert!(
        message.contains("A team admin can enable it in Coralogix."),
        "{message}"
    );
}

#[tokio::test]
async fn views_list_posts_app_time_range_and_paging() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/views/v1")))
        .and(body_json(json!({
            "appName": "web-shop",
            "timeRange": fixed_range_json(),
            "pageSize": 2,
            "pageToken": "tok",
            "filters": [{"stringFilter": {"field": "STRING_FILTER_FIELD_PLATFORM", "condition": {"values": ["web"], "operator": "STRING_CONDITION_OPERATOR_EQUAL"}}}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "views": [{"view": "/", "sessions": "18420"}, {"view": "/pricing", "sessions": "9870"}],
            "nextPageToken": "eyJvIjo1MH0",
            "costly": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = write_json(
        "views",
        &json!({"filters": [{"stringFilter": {"field": "STRING_FILTER_FIELD_PLATFORM", "condition": {"values": ["web"], "operator": "STRING_CONDITION_OPERATOR_EQUAL"}}}]}),
    );
    let input = QueryInput {
        app_name: Some("web-shop".into()),
        time_range: Some(FIXED_RANGE),
        page_size: Some(2),
        page_token: Some("tok".into()),
        ..Default::default()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list_query(
        &targets,
        QueryKind::Views,
        file.to_str(),
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("views list should succeed");
    std::fs::remove_file(file).ok();
}

#[tokio::test]
async fn events_list_sends_source_and_fixed_time_range() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/events/v1")))
        .and(body_json(json!({
            "appName": "web-shop",
            "view": "/cart",
            "source": "EVENT_SOURCE_AUTO",
            "timeRange": fixed_range_json()
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "events": [{"eventId": "ev_Y2xpY2s6R290IGl0", "event": {"source": "EVENT_SOURCE_AUTO", "name": "Got it"},
                        "count": "5", "deadClicks": "0", "deadClicksPct": 0, "rageClicks": "2", "rageClicksPct": 40}],
            "nextPageToken": "",
            "costly": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let input = QueryInput {
        app_name: Some("web-shop".into()),
        view: Some("/cart".into()),
        source: Some(EventSource::Auto),
        time_range: Some(FIXED_RANGE),
        ..Default::default()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list_query(
        &targets,
        QueryKind::Events,
        None,
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("events list should succeed");
}

#[tokio::test]
async fn events_get_posts_to_the_event_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/events/v1/ev_Y2xpY2s6R290IGl0")))
        .and(body_partial_json(json!({"appName": "web-shop", "view": "/cart"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "event": {"eventId": "ev_Y2xpY2s6R290IGl0", "event": {"source": "EVENT_SOURCE_AUTO", "name": "Got it"},
                      "count": "5", "deadClicks": "0", "deadClicksPct": 0, "rageClicks": "2", "rageClicksPct": 40},
            "costly": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let input = QueryInput {
        app_name: Some("web-shop".into()),
        view: Some("/cart".into()),
        ..Default::default()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_events_get(
        &targets,
        "ev_Y2xpY2s6R290IGl0",
        None,
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("events get should succeed");
}

#[tokio::test]
async fn saved_lists_use_each_kind_path_and_paging_params() {
    let server = MockServer::start().await;
    for (kind_path, key) in [
        ("funnels", "funnels"),
        ("pathways", "pathways"),
        ("heatmaps", "heatmaps"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("{BASE}/{kind_path}/v1")))
            .and(query_param("app_names", "web-shop"))
            .and(query_param("page_size", "2"))
            .and(query_param("page_token", "next"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                key: [saved_funnel()],
                "nextPageToken": "more",
                "totalSize": 3
            })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let page = PageArgs {
        page_size: Some(2),
        page_token: Some("next".into()),
    };
    for kind in [
        SavedObjectKind::Funnel,
        SavedObjectKind::Pathway,
        SavedObjectKind::Heatmap,
    ] {
        run_saved_list(
            &targets,
            kind,
            &["web-shop".to_string()],
            &page,
            OutputFormat::Text,
        )
        .await
        .expect("saved list should succeed");
    }
}

#[tokio::test]
async fn saved_list_merges_profiles() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    for server in [&first, &second] {
        Mock::given(method("GET"))
            .and(path(format!("{BASE}/heatmaps/v1")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "heatmaps": [{"id": "43", "appName": "web-shop", "viewName": "/checkout", "name": "Checkout clicks",
                              "mode": "HEATMAP_MODE_CLICK_MAP", "version": 1}],
                "nextPageToken": "",
                "totalSize": 1
            })))
            .expect(1)
            .mount(server)
            .await;
    }
    let targets = vec![
        common::test_target("prod", &first.uri()),
        common::test_target("staging", &second.uri()),
    ];
    run_saved_list(
        &targets,
        SavedObjectKind::Heatmap,
        &[],
        &PageArgs::default(),
        OutputFormat::Text,
    )
    .await
    .expect("multi-profile list should succeed");
}

#[tokio::test]
async fn saved_get_create_and_delete() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/funnels/v1/42")))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved_funnel()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/pathways/v1")))
        .and(body_json(json!({"appName": "web-shop", "name": "After cart", "view": "/cart", "timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "44", "appName": "web-shop", "name": "After cart", "view": "/cart",
            "direction": "PATHWAY_DIRECTION_AFTER", "pagesCount": 5, "topN": 20, "version": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/heatmaps/v1/43")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_saved_get(&targets, SavedObjectKind::Funnel, "42", OutputFormat::Text)
        .await
        .expect("get should succeed");

    let file = write_json(
        "pathway_create",
        &json!({"appName": "web-shop", "name": "After cart", "view": "/cart", "timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}}),
    );
    run_saved_write(
        &targets,
        SavedObjectKind::Pathway,
        file.to_str().unwrap(),
        None,
        OutputFormat::Json,
    )
    .await
    .expect("create should succeed");
    std::fs::remove_file(file).ok();

    run_saved_delete(&targets, SavedObjectKind::Heatmap, "43")
        .await
        .expect("delete should succeed");
}

#[tokio::test]
async fn update_sends_mask_and_strips_read_only_fields() {
    let server = MockServer::start().await;
    let mut renamed = saved_funnel();
    renamed["name"] = json!("Renamed");
    renamed["version"] = json!(2);
    let expected_body = json!({
        "name": "Renamed",
        "definition": saved_funnel()["definition"],
        "filters": [],
        "timeRange": {"relativeMinutes": "1440"}
    });
    Mock::given(method("PATCH"))
        .and(path(format!("{BASE}/funnels/v1/42")))
        .and(query_param(
            "updateMask",
            "definition,filters,name,timeRange",
        ))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(&renamed))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("{BASE}/funnels/v1/42")))
        .and(query_param("updateMask", "name"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&renamed))
        .expect(1)
        .mount(&server)
        .await;

    let file = write_json("funnel_update", &renamed);
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_saved_write(
        &targets,
        SavedObjectKind::Funnel,
        file.to_str().unwrap(),
        Some(("42", None)),
        OutputFormat::Json,
    )
    .await
    .expect("update with derived mask should succeed");
    run_saved_write(
        &targets,
        SavedObjectKind::Funnel,
        file.to_str().unwrap(),
        Some(("42", Some("name"))),
        OutputFormat::Text,
    )
    .await
    .expect("update with explicit mask should succeed");
    std::fs::remove_file(file).ok();
}

#[tokio::test]
async fn funnel_query_runs_a_saved_funnel() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1:queryConversion")))
        .and(body_json(json!({"funnelId": "42", "mode": "FUNNEL_MODE_USERS"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "conversion": {
                "appName": "web-shop", "mode": "FUNNEL_MODE_USERS",
                "summary": {"enteredCount": "30122", "convertedCount": "21877", "convertedPct": 72.6,
                            "droppedCount": "8245", "droppedPct": 27.4,
                            "biggestDropOff": {"fromStepId": "s1", "toStepId": "s2", "droppedCount": "8245", "droppedPctOfPrevious": 27.4}},
                "steps": [
                    {"stepId": "s1", "label": "Viewed cart", "count": "30122", "convertedPctOfTotal": 100, "convertedPctOfPrevious": 100,
                     "droppedCount": "0", "droppedPctOfTotal": 0, "droppedPctOfPrevious": 0},
                    {"stepId": "s2", "label": "Clicked pay", "count": "21877", "convertedPctOfTotal": 72.6, "convertedPctOfPrevious": 72.6,
                     "droppedCount": "8245", "droppedPctOfTotal": 27.4, "droppedPctOfPrevious": 27.4, "avgTimeFromPrevious": "192s"}
                ],
                "groups": []
            },
            "costly": false
        })))
        .expect(2)
        .mount(&server)
        .await;

    let input = QueryInput {
        saved_id: Some("42".into()),
        funnel_mode: Some(coralogix_cli::commands::product_analytics::api::FunnelMode::Users),
        ..Default::default()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Text, OutputFormat::Json] {
        run_query(&targets, QueryKind::FunnelConversion, None, &input, output)
            .await
            .expect("funnel query should succeed");
    }
}

#[tokio::test]
async fn funnel_query_with_breakdown_groups_renders() {
    let server = MockServer::start().await;
    let step = json!({"stepId": "s1", "count": "10", "convertedPctOfTotal": 100, "convertedPctOfPrevious": 100,
                      "droppedCount": "0", "droppedPctOfTotal": 0, "droppedPctOfPrevious": 0});
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1:queryConversion")))
        .and(body_partial_json(json!({"appName": "web-shop", "breakdown": {"field": "BREAKDOWN_FIELD_PLATFORM", "limit": 5}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "conversion": {
                "appName": "web-shop", "mode": "FUNNEL_MODE_USERS", "steps": [],
                "groups": [{"group": "web", "steps": [step]}, {"group": "ios", "steps": [step]}],
                "groupComparison": {"bestConverting": {"group": "web", "convertedPct": 100},
                                    "worstConverting": {"group": "ios", "convertedPct": 100}, "conversionGapPct": 0}
            },
            "costly": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = write_json(
        "funnel_inline",
        &json!({
            "definition": {"mode": "FUNNEL_MODE_USERS", "steps": [{"stepId": "s1", "conditions": [{"view": {}}]}]},
            "breakdown": {"field": "BREAKDOWN_FIELD_PLATFORM", "limit": 5}
        }),
    );
    let input = QueryInput {
        app_name: Some("web-shop".into()),
        ..Default::default()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_query(
        &targets,
        QueryKind::FunnelConversion,
        file.to_str(),
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("inline funnel query should succeed");
    std::fs::remove_file(file).ok();
}

#[tokio::test]
async fn funnel_matches_lists_users() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1:listMatches")))
        .and(body_json(json!({
            "funnelId": "42",
            "stepId": "s2",
            "outcome": "FUNNEL_OUTCOME_DROPPED",
            "pageSize": 100
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "appName": "web-shop", "stepId": "s2", "outcome": "FUNNEL_OUTCOME_DROPPED", "mode": "FUNNEL_MODE_USERS",
            "users": [{"userId": "a3f9c1d27b4e8f60a1c2d3e4f5061728", "exampleSessionId": "5b2e9f10",
                       "hasSessionReplay": true, "platform": "web", "os": "macOS", "country": "Germany"}],
            "sessions": [], "accounts": [], "nextPageToken": ""
        })))
        .expect(1)
        .mount(&server)
        .await;

    let input = QueryInput {
        saved_id: Some("42".into()),
        step_id: Some("s2".into()),
        outcome: Some(FunnelOutcome::Dropped),
        page_size: Some(100),
        ..Default::default()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list_query(
        &targets,
        QueryKind::FunnelMatches,
        None,
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("funnel matches should succeed");
}

#[tokio::test]
async fn pathway_and_heatmap_inline_queries() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/pathways/v1:queryPaths")))
        .and(body_json(json!({
            "appName": "docs", "view": "/setup", "direction": "PATHWAY_DIRECTION_BEFORE", "pagesCount": 3,
            "timeRange": fixed_range_json()
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pathwayPaths": {"appName": "docs", "view": "/setup", "direction": "PATHWAY_DIRECTION_BEFORE",
                             "paths": [{"pages": ["/", "/setup"], "count": "41"}]},
            "costly": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/heatmaps/v1:queryTopElements")))
        .and(body_json(json!({"heatmapId": "43", "topN": 10})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "heatmapData": {"appName": "web-app", "view": "explore", "actionNameFilter": "ACTION_NAME_FILTER_ALL_CLICKS",
                            "elements": [{"eventId": "ev_Y2xpY2s6cnVu", "name": "query-builder-run", "clicks": "12420",
                                          "deadClicksPct": 0, "rageClicksPct": 1}]},
            "costly": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let pathway = QueryInput {
        app_name: Some("docs".into()),
        view: Some("/setup".into()),
        direction: Some(PathwayDirection::Before),
        pages_count: Some(3),
        time_range: Some(FIXED_RANGE),
        ..Default::default()
    };
    run_query(
        &targets,
        QueryKind::PathwayPaths,
        None,
        &pathway,
        OutputFormat::Text,
    )
    .await
    .expect("pathway query should succeed");
    let heatmap = QueryInput {
        saved_id: Some("43".into()),
        top_n: Some(10),
        ..Default::default()
    };
    run_query(
        &targets,
        QueryKind::HeatmapTopElements,
        None,
        &heatmap,
        OutputFormat::Toon,
    )
    .await
    .expect("heatmap query should succeed");
}

#[tokio::test]
async fn settings_get_and_update() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/settings/v1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("{BASE}/settings/v1")))
        .and(query_param("updateMask", "enabled"))
        .and(body_json(json!({"enabled": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"enabled": true})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_settings_get(&targets, OutputFormat::Text)
        .await
        .expect("settings get should succeed");
    run_settings_update(&targets, true, OutputFormat::Json)
        .await
        .expect("settings update should succeed");
}
