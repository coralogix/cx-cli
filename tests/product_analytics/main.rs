#[path = "../common/mod.rs"]
mod common;

use serde_json::{json, Value};
use wiremock::matchers::{body_json, body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::product_analytics::api::{
    ActionNameFilter, ActionType, FunnelGroupBy, FunnelOutcome, PathwayDirection, QueryInput,
    QueryKind, SavedObjectKind,
};
use coralogix_cli::commands::product_analytics::{
    run_list_query, run_query, run_saved_delete, run_saved_get, run_saved_list, run_saved_write,
    run_settings_get, run_settings_update,
};
use coralogix_cli::config::OutputFormat;

const BASE: &str = "/product-analytics";
const FIXED_RANGE: (i64, i64) = (1_763_251_200, 1_763_337_600);

fn fixed_input() -> QueryInput {
    QueryInput {
        time_range: Some(FIXED_RANGE),
        ..Default::default()
    }
}

fn funnel_steps() -> Value {
    json!([
        {"stepId": "s1", "conditions": [{"page": {"match": {"values": ["login/user"], "operator": "EQUAL"}}}]},
        {"stepId": "s2", "conditions": [{"page": {"match": {"values": ["explore"], "operator": "EQUAL"}}}]}
    ])
}

fn saved_funnel(time_range: Value) -> Value {
    json!({
        "id": "42",
        "appName": "web-shop",
        "name": "Checkout funnel",
        "steps": funnel_steps(),
        "filters": [{"field": "PLATFORM", "stringCondition": {"values": ["browser"]}}],
        "groupBy": "GROUP_BY_CX_USER_ID",
        "createdAt": "1783433258498",
        "timeRange": time_range
    })
}

fn funnel_response() -> Value {
    json!({"groups": [{"steps": [
        {"stepNumber": 1, "sessionCount": "30122", "conversionRate": 1.0},
        {"stepNumber": 2, "sessionCount": "21877", "conversionRate": 0.726, "avgTimeFromPrevMs": "192000"}
    ]}]})
}

fn write_json(name: &str, body: &Value) -> std::path::PathBuf {
    let file = std::env::temp_dir().join(format!("cx_pa_test_{}_{name}.json", std::process::id()));
    std::fs::write(&file, serde_json::to_vec(body).unwrap()).unwrap();
    file
}

async fn mount_saved_search(server: &MockServer, kind: &str, id: i64, saved: Value) {
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/{kind}/v1/saved/search")))
        .and(body_json(json!({"ids": {"ids": [id]}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({kind: [saved]})))
        .expect(1..)
        .mount(server)
        .await;
}

#[tokio::test]
async fn applications_list_posts_the_range_in_seconds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/applications/v1")))
        .and(body_json(
            json!({"fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apps": [{"appName": "web-shop", "sessionCount": "18420", "crashFreeSessionCount": "18391",
                      "totalUsers": "6210", "crashFreeUserCount": "6198", "platform": "browser"}, {"appName": "ios-shop"}]
        })))
        .expect(3)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        run_list_query(
            &targets,
            QueryKind::Applications,
            None,
            None,
            &fixed_input(),
            output,
        )
        .await
        .expect("applications list should succeed");
    }
}

#[tokio::test]
async fn applications_list_empty_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/applications/v1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list_query(
        &targets,
        QueryKind::Applications,
        None,
        None,
        &fixed_input(),
        OutputFormat::Text,
    )
    .await
    .expect("empty applications list should succeed");
}

#[tokio::test]
async fn service_error_carries_the_server_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1/saved/search")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": "INVALID_ARGUMENT",
            "message": "Failed to deserialize the JSON body"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = run_saved_list(&targets, SavedObjectKind::Funnel, &[], OutputFormat::Json)
        .await
        .expect_err("400 must fail");
    let message = format!("{error:#}");
    assert!(message.contains("400"), "{message}");
    assert!(
        message.contains("Failed to deserialize the JSON body"),
        "{message}"
    );
}

#[tokio::test]
async fn views_list_posts_app_range_offset_and_file_filters() {
    let server = MockServer::start().await;
    let filters = json!([{"field": "PLATFORM", "stringCondition": {"values": ["browser"], "operator": "EQUAL"}}]);
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/views/v1")))
        .and(body_json(json!({
            "appNames": ["web-shop"],
            "fromTimestamp": FIXED_RANGE.0,
            "toTimestamp": FIXED_RANGE.1,
            "offset": 1000,
            "filters": filters
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "views": [{"viewName": "explore", "sessionCount": "35814"}, {"viewName": "login/user"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = write_json("views", &json!({"filters": filters}));
    let input = QueryInput {
        app_name: Some("web-shop".into()),
        offset: Some(1000),
        ..fixed_input()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list_query(
        &targets,
        QueryKind::Views,
        None,
        file.to_str(),
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("views list should succeed");
    std::fs::remove_file(file).ok();
}

#[tokio::test]
async fn actions_list_sends_click_or_custom() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/actions/v1")))
        .and(body_json(json!({
            "appNames": ["web-shop"],
            "fromTimestamp": FIXED_RANGE.0,
            "toTimestamp": FIXED_RANGE.1,
            "click": {"actionNameFilter": "UNNAMED_ONLY"}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "actions": [{"eventType": "click", "eventTarget": "Log in with SSO", "sessionCount": "4459"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/actions/v1")))
        .and(body_partial_json(json!({"custom": {}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "actions": [{"eventType": "custom", "eventTarget": "App loaded", "sessionCount": "118224"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let click = QueryInput {
        app_name: Some("web-shop".into()),
        action_type: Some(ActionType::Click),
        action_name_filter: Some(ActionNameFilter::Unnamed),
        ..fixed_input()
    };
    run_list_query(
        &targets,
        QueryKind::Actions,
        None,
        None,
        &click,
        OutputFormat::Text,
    )
    .await
    .expect("click actions should succeed");
    let custom = QueryInput {
        app_name: Some("web-shop".into()),
        action_type: Some(ActionType::Custom),
        ..fixed_input()
    };
    run_list_query(
        &targets,
        QueryKind::Actions,
        None,
        None,
        &custom,
        OutputFormat::Json,
    )
    .await
    .expect("custom actions should succeed");
}

#[tokio::test]
async fn filter_values_sends_the_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/filter-values/v1")))
        .and(body_json(json!({
            "appNames": ["web-shop"],
            "fromTimestamp": FIXED_RANGE.0,
            "toTimestamp": FIXED_RANGE.1,
            "filterField": "URL_DOMAIN"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{"value": "coralogix.com", "count": "102853"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let input = QueryInput {
        app_name: Some("web-shop".into()),
        filter_field: Some("URL_DOMAIN".into()),
        ..fixed_input()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list_query(
        &targets,
        QueryKind::FilterValues,
        None,
        None,
        &input,
        OutputFormat::Toon,
    )
    .await
    .expect("filter values should succeed");
}

#[tokio::test]
async fn saved_lists_search_each_kind_by_app_names() {
    let server = MockServer::start().await;
    for kind in ["funnels", "pathways", "heatmaps"] {
        Mock::given(method("POST"))
            .and(path(format!("{BASE}/{kind}/v1/saved/search")))
            .and(body_json(json!({"appNames": {"values": ["web-shop"]}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                kind: [saved_funnel(json!({"relativeMinutes": "1440"}))]
            })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for kind in [
        SavedObjectKind::Funnel,
        SavedObjectKind::Pathway,
        SavedObjectKind::Heatmap,
    ] {
        run_saved_list(
            &targets,
            kind,
            &["web-shop".to_string()],
            OutputFormat::Text,
        )
        .await
        .expect("saved list should succeed");
    }
}

#[tokio::test]
async fn saved_list_merges_profiles_and_all_apps_sends_empty_values() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    for server in [&first, &second] {
        Mock::given(method("POST"))
            .and(path(format!("{BASE}/heatmaps/v1/saved/search")))
            .and(body_json(json!({"appNames": {"values": []}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "heatmaps": [{"id": "3", "appName": "web-shop", "viewName": "/checkout", "name": "Checkout clicks",
                              "mode": "CLICK_MAP", "timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}}]
            })))
            .expect(1)
            .mount(server)
            .await;
    }
    let targets = vec![
        common::test_target("prod", &first.uri()),
        common::test_target("staging", &second.uri()),
    ];
    run_saved_list(&targets, SavedObjectKind::Heatmap, &[], OutputFormat::Text)
        .await
        .expect("multi-profile list should succeed");
}

#[tokio::test]
async fn saved_get_create_update_and_delete() {
    let server = MockServer::start().await;
    mount_saved_search(
        &server,
        "funnels",
        42,
        saved_funnel(json!({"relativeMinutes": "1440"})),
    )
    .await;
    let pathway = json!({"appName": "web-shop", "name": "After cart", "pageName": "/cart", "pagesCount": 5,
                         "limit": 20, "timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}});
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/pathways/v1/saved")))
        .and(body_json(&pathway))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "44"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("{BASE}/funnels/v1/saved/42")))
        .and(body_json(json!({
            "name": "Renamed",
            "steps": funnel_steps(),
            "filters": [{"field": "PLATFORM", "stringCondition": {"values": ["browser"]}}],
            "groupBy": "GROUP_BY_CX_USER_ID",
            "timeRange": {"relativeMinutes": "1440"}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("{BASE}/heatmaps/v1/saved/43")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_saved_get(&targets, SavedObjectKind::Funnel, "42", OutputFormat::Text)
        .await
        .expect("get should succeed");

    let create_file = write_json("pathway_create", &pathway);
    run_saved_write(
        &targets,
        SavedObjectKind::Pathway,
        create_file.to_str().unwrap(),
        None,
        OutputFormat::Json,
    )
    .await
    .expect("create should succeed");
    std::fs::remove_file(create_file).ok();

    let mut edited = saved_funnel(json!({"relativeMinutes": "1440"}));
    edited["name"] = json!("Renamed");
    let update_file = write_json("funnel_update", &edited);
    run_saved_write(
        &targets,
        SavedObjectKind::Funnel,
        update_file.to_str().unwrap(),
        Some("42"),
        OutputFormat::Json,
    )
    .await
    .expect("update should succeed");
    std::fs::remove_file(update_file).ok();

    run_saved_delete(&targets, SavedObjectKind::Heatmap, "43")
        .await
        .expect("delete should succeed");
}

#[tokio::test]
async fn saved_get_of_a_missing_id_fails() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/pathways/v1/saved/search")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = run_saved_get(&targets, SavedObjectKind::Pathway, "7", OutputFormat::Json)
        .await
        .expect_err("a missing pathway must fail");
    assert!(format!("{error:#}").contains("pathway 7 not found"));
    assert!(
        run_saved_get(&targets, SavedObjectKind::Pathway, "x", OutputFormat::Json)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn funnel_query_runs_a_saved_funnel_with_its_fixed_range() {
    let server = MockServer::start().await;
    mount_saved_search(
        &server,
        "funnels",
        42,
        saved_funnel(
            json!({"fixed": {"fromTimestamp": "1763251200", "toTimestamp": "1763337600"}}),
        ),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1/query")))
        .and(body_json(json!({
            "query": {
                "appNames": ["web-shop"],
                "fromTimestamp": FIXED_RANGE.0,
                "toTimestamp": FIXED_RANGE.1,
                "steps": funnel_steps(),
                "filters": [{"field": "PLATFORM", "stringCondition": {"values": ["browser"]}}]
            },
            "groupBy": "GROUP_BY_CX_USER_ID"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(funnel_response()))
        .expect(2)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    for output in [OutputFormat::Text, OutputFormat::Json] {
        run_query(
            &targets,
            QueryKind::Funnel,
            Some("42"),
            None,
            &QueryInput::default(),
            output,
        )
        .await
        .expect("saved funnel query should succeed");
    }
}

#[tokio::test]
async fn funnel_query_flags_override_the_saved_range_and_group_by() {
    let server = MockServer::start().await;
    mount_saved_search(
        &server,
        "funnels",
        42,
        saved_funnel(json!({"relativeMinutes": "1440"})),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1/query")))
        .and(body_partial_json(json!({
            "query": {"fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1},
            "groupBy": "GROUP_BY_ACCOUNT_ID"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(funnel_response()))
        .expect(1)
        .mount(&server)
        .await;

    let input = QueryInput {
        group_by: Some(FunnelGroupBy::Accounts),
        ..fixed_input()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_query(
        &targets,
        QueryKind::Funnel,
        Some("42"),
        None,
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("saved funnel query with flags should succeed");
}

#[tokio::test]
async fn funnel_query_of_a_missing_saved_funnel_fails() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1/saved/search")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let error = run_query(
        &targets,
        QueryKind::Funnel,
        Some("9"),
        None,
        &QueryInput::default(),
        OutputFormat::Json,
    )
    .await
    .expect_err("a missing funnel must fail");
    assert!(format!("{error:#}").contains("funnel 9 not found"));
}

#[tokio::test]
async fn inline_funnel_query_with_breakdown_groups_renders() {
    let server = MockServer::start().await;
    let step = json!({"stepNumber": 1, "sessionCount": "10", "conversionRate": 1.0});
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1/query")))
        .and(body_json(json!({
            "query": {"appNames": ["web-shop"], "fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1,
                      "steps": funnel_steps()},
            "breakdown": {"field": "PLATFORM", "limit": 5}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "groups": [{"groupValue": "web", "steps": [step]}, {"groupValue": "ios", "steps": [step]}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = write_json(
        "funnel_inline",
        &json!({"query": {"appNames": ["other"], "steps": funnel_steps()}, "breakdown": {"field": "PLATFORM", "limit": 5}}),
    );
    let input = QueryInput {
        app_name: Some("web-shop".into()),
        ..fixed_input()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_query(
        &targets,
        QueryKind::Funnel,
        None,
        file.to_str(),
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("inline funnel query should succeed");
    std::fs::remove_file(file).ok();
}

#[tokio::test]
async fn matching_users_of_a_saved_funnel_end_at_its_last_step() {
    let server = MockServer::start().await;
    mount_saved_search(
        &server,
        "funnels",
        42,
        saved_funnel(json!({"relativeMinutes": "1440"})),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/funnels/v1/matching-users")))
        .and(body_json(json!({
            "query": {
                "appNames": ["web-shop"],
                "fromTimestamp": FIXED_RANGE.0,
                "toTimestamp": FIXED_RANGE.1,
                "steps": funnel_steps(),
                "filters": [{"field": "PLATFORM", "stringCondition": {"values": ["browser"]}}]
            },
            "mode": "DROPOFF",
            "startStep": 1,
            "endStep": 2,
            "limit": 100,
            "offset": 100
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "users": [{"userId": "a3f9c1d2", "userName": "Dana", "userEmail": "dana@example.com",
                       "exampleSessionId": "5b2e9f10", "hasSessionReplay": true, "platform": "browser",
                       "os": "macos", "countryName": "Germany"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let input = QueryInput {
        outcome: Some(FunnelOutcome::Dropped),
        limit: Some(100),
        offset: Some(100),
        ..fixed_input()
    };
    let targets = vec![common::test_target("test-profile", &server.uri())];
    run_list_query(
        &targets,
        QueryKind::MatchingUsers,
        Some("42"),
        None,
        &input,
        OutputFormat::Text,
    )
    .await
    .expect("matching users should succeed");
}

#[tokio::test]
async fn pathway_queries_saved_and_inline() {
    let server = MockServer::start().await;
    mount_saved_search(
        &server,
        "pathways",
        44,
        json!({"id": "44", "appName": "docs", "name": "From setup", "pageName": "/setup",
               "pagesCount": 4, "limit": 8, "mode": "PATHWAYS_MODE_BACKWARD",
               "timeRange": {"fixed": {"fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1}}}),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/pathways/v1/query")))
        .and(body_json(json!({
            "appNames": ["docs"], "pageName": "/setup", "pagesCount": 4, "limit": 3,
            "mode": "PATHWAYS_MODE_BACKWARD", "fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pathways": [{"pages": ["/", "/setup"], "count": "41"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/pathways/v1/query")))
        .and(body_json(json!({
            "appNames": ["docs"], "pageName": "/tools", "mode": "PATHWAYS_MODE_FORWARD",
            "pagesCount": 3, "fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let saved = QueryInput {
        limit: Some(3),
        ..Default::default()
    };
    run_list_query(
        &targets,
        QueryKind::Pathways,
        Some("44"),
        None,
        &saved,
        OutputFormat::Text,
    )
    .await
    .expect("saved pathway query should succeed");
    let inline = QueryInput {
        app_name: Some("docs".into()),
        view: Some("/tools".into()),
        direction: Some(PathwayDirection::Forward),
        pages_count: Some(3),
        ..fixed_input()
    };
    run_list_query(
        &targets,
        QueryKind::Pathways,
        None,
        None,
        &inline,
        OutputFormat::Text,
    )
    .await
    .expect("inline pathway query should succeed");
}

#[tokio::test]
async fn heatmap_clicks_and_scroll_map_of_a_saved_heatmap() {
    let server = MockServer::start().await;
    mount_saved_search(
        &server,
        "heatmaps",
        43,
        json!({"id": "43", "appName": "web-app", "name": "Explore", "viewName": "explore",
               "mode": "TOP_ELEMENTS", "snapshotId": "s", "actionNameFilter": "NAMED_ONLY",
               "timeRange": {"fixed": {"fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1}}}),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/clicks/v1")))
        .and(body_json(json!({
            "appName": "web-app", "pageName": "explore", "actionNameFilter": "NAMED_ONLY", "topK": 10,
            "fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "clicks": [{"clickName": "run", "cxActionName": "query-builder-run", "clickCount": "12420",
                        "deadClickCount": "2", "fingerprint": "BUTTON",
                        "coordinates": [{"coordinate": {"height": 10, "width": 20}, "count": "4"}]}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{BASE}/scroll-map/v1")))
        .and(body_json(json!({
            "appName": "web-app", "pageName": "explore",
            "fromTimestamp": FIXED_RANGE.0, "toTimestamp": FIXED_RANGE.1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pageCovers": [{"userCount": "8728"}, {"pagePercentage": 100, "userCount": "8723"}],
            "avgViewportHeight": 957
        })))
        .expect(2)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];
    let clicks = QueryInput {
        top_n: Some(10),
        ..Default::default()
    };
    run_list_query(
        &targets,
        QueryKind::Clicks,
        Some("43"),
        None,
        &clicks,
        OutputFormat::Text,
    )
    .await
    .expect("heatmap query should succeed");
    for output in [OutputFormat::Text, OutputFormat::Toon] {
        run_query(
            &targets,
            QueryKind::ScrollMap,
            Some("43"),
            None,
            &QueryInput::default(),
            output,
        )
        .await
        .expect("scroll map should succeed");
    }
}

#[tokio::test]
async fn settings_get_and_update() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{BASE}/settings/v1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"enabled": true})))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("{BASE}/settings/v1")))
        .and(body_json(json!({"enabled": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
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
