use std::process::Command;

use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn base_command(home: &TempDir) -> Command {
    let mut cx = Command::new(env!("CARGO_BIN_EXE_cx"));
    cx.env_clear()
        .env("CX_HOME", home.path())
        .env("CX_NO_UPDATE_NOTIFIER", "1")
        .env("CX_NO_SKILLS_AUTO_UPDATE", "1");
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        cx.env("SystemRoot", system_root);
    }
    cx
}

#[tokio::test]
async fn telemetry_v1_posts_single_span_to_loopback() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let home = TempDir::new().expect("temp home");
    let mut cx = base_command(&home);
    cx.env("CX_TELEMETRY", "1")
        .env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
        .env("CX_OTLP_API_KEY", "local-test-key")
        .arg("schema");

    let output = cx.output().expect("run cx");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn telemetry_v1_skips_export_when_do_not_track() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let home = TempDir::new().expect("temp home");
    let mut cx = base_command(&home);
    cx.env("CX_TELEMETRY", "1")
        .env("DO_NOT_TRACK", "1")
        .env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
        .env("CX_OTLP_API_KEY", "local-test-key")
        .arg("schema");

    let output = cx.output().expect("run cx");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn telemetry_v1_skips_export_when_cx_telemetry_unset() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let home = TempDir::new().expect("temp home");
    let mut cx = base_command(&home);
    // Endpoint alone must not enable export; CX_TELEMETRY stays unset.
    cx.env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
        .env("CX_OTLP_API_KEY", "local-test-key")
        .arg("schema");

    let output = cx.output().expect("run cx");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn telemetry_v1_skips_export_when_config_opts_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let home = TempDir::new().expect("temp home");
    let cx_dir = home.path().join(".cx");
    std::fs::create_dir_all(&cx_dir).expect("cx dir");
    std::fs::write(cx_dir.join("config.toml"), "telemetry = false\n").expect("config");

    let mut cx = base_command(&home);
    cx.env("CX_TELEMETRY", "1")
        .env("CX_OTLP_ENDPOINT", format!("{}/v1/traces", server.uri()))
        .env("CX_OTLP_API_KEY", "local-test-key")
        .arg("schema");

    let output = cx.output().expect("run cx");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
