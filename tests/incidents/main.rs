#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::incidents::{run_list, ListIncidentsOptions};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_incidents_posts_filter_arrays_and_paginates() {
    let server = MockServer::start().await;

    let endpoint = "/mgmt/openapi/5/incidents/incidents/v1";

    Mock::given(method("POST"))
        .and(path(endpoint))
        .and(body_json(json!({
            "filter": {
                "status": ["INCIDENT_STATUS_TRIGGERED"],
                "severity": ["INCIDENT_SEVERITY_CRITICAL"],
                "assignee": ["user-1"]
            },
            "pagination": {
                "pageSize": 2
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "incidents": [
                {"id": "inc-1", "name": "First"},
                {"id": "inc-2", "name": "Second"}
            ],
            "pagination": {
                "nextPageToken": "token-2"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(endpoint))
        .and(body_json(json!({
            "filter": {
                "status": ["INCIDENT_STATUS_TRIGGERED"],
                "severity": ["INCIDENT_SEVERITY_CRITICAL"],
                "assignee": ["user-1"]
            },
            "pagination": {
                "pageSize": 2,
                "pageToken": "token-2"
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "incidents": [
                {"id": "inc-3", "name": "Third"},
                {"id": "inc-4", "name": "Fourth"}
            ],
            "pagination": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(
        &[target],
        ListIncidentsOptions {
            statuses: vec!["triggered".to_string()],
            severities: vec!["critical".to_string()],
            assignees: vec!["user-1".to_string()],
            page_size: 2,
            limit: Some(3),
            ..Default::default()
        },
        OutputFormat::Json,
    )
    .await
    .expect("run_list should succeed");
}
