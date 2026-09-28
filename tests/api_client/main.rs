use std::time::Duration;

use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::api_client::CxClient;

fn init_tls() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
}

#[tokio::test]
async fn attaches_gateway_metric_headers() {
    init_tls();
    let server = MockServer::start().await;
    let sdk_version = concat!("cx-cli-", env!("CARGO_PKG_VERSION"));
    Mock::given(method("GET"))
        .and(path("/test"))
        .and(header("x-cx-sdk-version", sdk_version))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();

    client.get::<Value>("/test", &[]).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].headers["user-agent"]
        .to_str()
        .unwrap()
        .starts_with("cx-cli/"));
    assert!(requests[0].headers.get("x-cx-cli-metadata").is_none());
}

#[tokio::test]
async fn honors_configured_request_timeout() {
    init_tls();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(1)))
        .mount(&server)
        .await;

    let client =
        CxClient::with_timeout(server.uri(), "test-key", Some(Duration::from_millis(10))).unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    assert!(matches!(err, coralogix_cli::error::CxError::Timeout));
}

#[tokio::test]
async fn error_401_unauthorized() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Invalid or expired API key"),
        "expected auth error, got: {msg}"
    );
}

#[tokio::test]
async fn error_403_with_message() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"message": "missing dashboards:write scope"})),
        )
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("missing dashboards:write scope"),
        "expected server message in error, got: {msg}"
    );
    assert!(
        !msg.contains("Check your API key's scopes"),
        "scope hint should not be appended when server provides detail, got: {msg}"
    );
    assert!(
        msg.contains("Permission denied"),
        "expected permission error, got: {msg}"
    );
}

#[tokio::test]
async fn error_403_quota_exceeded() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({"message": "quota exceeded"})),
        )
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("quota exceeded"),
        "expected server message in error, got: {msg}"
    );
    assert!(
        !msg.contains("Check your API key's scopes"),
        "scope hint should not be appended for quota errors, got: {msg}"
    );
}

#[tokio::test]
async fn error_403_without_message() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("You do not have permission for this operation"),
        "expected generic permission error, got: {msg}"
    );
    assert!(
        msg.contains("Check your API key's scopes"),
        "expected permission hint, got: {msg}"
    );
}

#[tokio::test]
async fn error_429_with_retry_after() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(429).append_header("Retry-After", "30"))
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Retry after 30 seconds"),
        "expected retry-after in error, got: {msg}"
    );
}

#[tokio::test]
async fn error_429_without_retry_after() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Wait and retry"),
        "expected generic rate-limit error, got: {msg}"
    );
}

#[tokio::test]
async fn error_500_with_json_message() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(json!({"message": "internal error"})),
        )
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("internal error"),
        "expected server message in error, got: {msg}"
    );
}

#[tokio::test]
async fn error_401_with_server_message() {
    init_tls();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"message": "Token expired"})))
        .mount(&server)
        .await;
    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Token expired"),
        "expected server message, got: {msg}"
    );
    assert!(msg.contains("cx profiles add"), "expected hint, got: {msg}");
}

#[tokio::test]
async fn error_429_with_message_and_retry_after() {
    init_tls();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(
            ResponseTemplate::new(429)
                .append_header("Retry-After", "30")
                .set_body_json(json!({"message": "Per-team quota exhausted"})),
        )
        .mount(&server)
        .await;
    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Per-team quota exhausted"),
        "expected server message, got: {msg}"
    );
    assert!(
        msg.contains("Retry after 30 seconds"),
        "expected retry hint, got: {msg}"
    );
}

#[tokio::test]
async fn error_404_catch_all() {
    init_tls();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"message": "resource not found"})),
        )
        .mount(&server)
        .await;
    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("resource not found"),
        "expected server message, got: {msg}"
    );
}

#[tokio::test]
async fn error_401_with_nested_error_message() {
    init_tls();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": {"message": "Bearer prefix missing"}})),
        )
        .mount(&server)
        .await;
    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Bearer prefix missing"),
        "expected nested server message, got: {msg}"
    );
    assert!(msg.contains("cx profiles add"), "expected hint, got: {msg}");
}

#[tokio::test]
async fn error_500_with_raw_body() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(500).set_body_string("something went wrong"))
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let err = client.get::<Value>("/test", &[]).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("something went wrong"),
        "expected raw body in error, got: {msg}"
    );
}

#[tokio::test]
async fn sdk_version_header_is_sent() {
    init_tls();
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .and(header(
            "x-cx-sdk-version",
            concat!("cx-cli-", env!("CARGO_PKG_VERSION")),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = CxClient::new(server.uri(), "test-key").unwrap();
    let _: Value = client.get("/test", &[]).await.unwrap();
}
