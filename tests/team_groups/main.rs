#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::team_groups::{run_list, run_users};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_team_groups_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "groups": [
            { "groupId": "grp-001", "name": "Engineering", "membersCount": 15, "description": "Eng team" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/team-groups/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn list_group_users_uses_v5_path() {
    let server = MockServer::start().await;

    let body = json!({
        "users": [
            { "userId": "uid-001", "username": "alice@example.com" }
        ],
        "totalCount": 1
    });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/aaa/team-groups/v2/grp-001/users/list",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_users(&[target], "grp-001", OutputFormat::Json)
        .await
        .expect("run_users should succeed");
}
