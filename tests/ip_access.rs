mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cx::commands::ip_access::run_get;
use cx::config::OutputFormat;

#[tokio::test]
async fn get_ip_access_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "ipAccessSettings": {
            "ipRanges": [
                { "cidr": "10.0.0.0/8", "description": "Internal" }
            ]
        }
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/aaa/team-sec-ip-access/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_get(&[target], OutputFormat::Json)
        .await
        .expect("run_get should succeed");
}
