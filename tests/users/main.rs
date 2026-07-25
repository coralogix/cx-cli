#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::users::run_search;
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn search_users_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/identity/whoami"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "team_id": 12345 })))
        .mount(&server)
        .await;

    let body = json!({
        "users": [
            { "userId": "u-001", "firstName": "Jane", "lastName": "Doe", "email": "jane@example.com", "status": "active" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/teams/v2/12345/search"))
        .and(query_param("pageSize", "300"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_search(&[target], None, None, None, None, OutputFormat::Json)
        .await
        .expect("run_search should succeed");
}
