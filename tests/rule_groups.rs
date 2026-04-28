mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::rule_groups::{run_list, run_usage_limits};
use cx::config::OutputFormat;

#[tokio::test]
async fn list_rule_groups_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "ruleGroups": [
            {
                "id": "rg-001",
                "name": "JSON Parser",
                "rules": [{"id": "r1"}, {"id": "r2"}],
                "enabled": true,
                "order": 1,
                "creator": "user@example.com"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/parsing-rules/rule-groups/v1"))
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
async fn list_rule_groups_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/parsing-rules/rule-groups/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty response");
}

#[tokio::test]
async fn usage_limits_from_mock() {
    let server = MockServer::start().await;

    let body = json!({ "maxRuleGroupsPerAccount": 100, "maxRulesPerRuleGroup": 15 });

    Mock::given(method("GET"))
        .and(path(
            "/mgmt/openapi/5/parsing-rules/rule-groups/v1/usage-limits",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_usage_limits(&[target], OutputFormat::Json)
        .await
        .expect("run_usage_limits should succeed");
}
