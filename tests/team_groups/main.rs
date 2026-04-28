#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::team_groups::run_list;
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
    run_list(&[target], None, None, OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}
