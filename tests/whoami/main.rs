#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::whoami::run_whoami;
use coralogix_cli::config::OutputFormat;

const WHOAMI: &str = "/identity/whoami";

fn whoami_body() -> serde_json::Value {
    json!({
        "team_id": 53623,
        "team_name": "c4c",
        "user_name": "alice@example.com",
        "team_url": "https://c4c.app.eu2.coralogix.com"
    })
}

#[tokio::test]
async fn whoami_reports_identity_from_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(WHOAMI))
        .respond_with(ResponseTemplate::new(200).set_body_json(whoami_body()))
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    run_whoami(&targets, OutputFormat::Json)
        .await
        .expect("run_whoami should succeed");
}

#[tokio::test]
async fn whoami_all_output_formats_render() {
    for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Toon] {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(WHOAMI))
            .respond_with(ResponseTemplate::new(200).set_body_json(whoami_body()))
            .expect(1)
            .mount(&server)
            .await;

        let targets = vec![common::test_target("test-profile", &server.uri())];

        run_whoami(&targets, format)
            .await
            .unwrap_or_else(|e| panic!("run_whoami should render {format:?}: {e:#}"));
    }
}

/// whoami is a single-profile check: selecting more than one profile is a usage
/// error, not a silent fan-out, and it must fail before issuing any request.
#[tokio::test]
async fn whoami_rejects_multiple_profiles() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(WHOAMI))
        .respond_with(ResponseTemplate::new(200).set_body_json(whoami_body()))
        .expect(0)
        .mount(&server)
        .await;

    let targets = vec![
        common::test_target("profile-a", &server.uri()),
        common::test_target("profile-b", &server.uri()),
    ];

    let err = run_whoami(&targets, OutputFormat::Json)
        .await
        .expect_err("multiple profiles should be rejected");
    let msg = format!("{err:#}");
    assert!(msg.contains("single profile"), "got: {msg}");
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

/// A 401 is the "bad credentials" case: it must surface as an error (non-zero
/// exit), with the auth guidance in the chain.
#[tokio::test]
async fn whoami_surfaces_auth_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(WHOAMI))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({ "message": "invalid token" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let targets = vec![common::test_target("test-profile", &server.uri())];

    let err = run_whoami(&targets, OutputFormat::Json)
        .await
        .expect_err("401 from the only profile should error");
    let chain = format!("{err:#}");
    assert!(
        chain.contains("authentication failed"),
        "auth failure should be classified, got: {chain}"
    );
}
