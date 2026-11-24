#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TRACE_PATH: &str = "/v1/traces";
const RELEASE_NOTICE: &str =
    "cx sends usage telemetry by default. Opt out with CX_TELEMETRY=off, DO_NOT_TRACK=1, or telemetry=false in ~/.cx/config.toml.";

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("cx-telemetry-symlink-{}", uuid::Uuid::new_v4())))
    }

    fn cx_dir(&self) -> PathBuf {
        self.0.join(".cx")
    }

    fn config_link(&self) -> PathBuf {
        self.cx_dir().join("config.toml")
    }

    fn notice_marker(&self) -> PathBuf {
        self.cx_dir()
            .join("telemetry-notice")
            .join("release-default-notice-shown")
    }

    fn link_config_to(&self, target: &Path) {
        fs::create_dir_all(self.cx_dir()).unwrap();
        let link = self.config_link();
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    fn write_target_file(&self, name: &str, contents: &str) -> PathBuf {
        fs::create_dir_all(&self.0).unwrap();
        let target = self.0.join(name);
        fs::write(&target, contents).unwrap();
        target
    }

    fn cx(&self, endpoint: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
        command
            .env_clear()
            .env("CX_HOME", &self.0)
            .env("CX_TELEMETRY_ENDPOINT", endpoint)
            .env("CX_TELEMETRY_API_KEY", "loopback-key")
            .env("CX_TELEMETRY_TEST_FORCE_RELEASE_NOTICE", "1")
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .env("CX_NO_SKILLS_AUTO_UPDATE", "1")
            .arg("--version");
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        command
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn collector() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

fn endpoint(server: &MockServer) -> String {
    format!("{}{TRACE_PATH}", server.uri())
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

fn assert_notice_marker_wellformed(path: &Path) {
    let metadata = fs::metadata(path).unwrap();
    assert!(metadata.is_file());
    assert_eq!(fs::read(path).unwrap(), b"shown-v1\n");
}

#[tokio::test]
async fn linked_config_without_telemetry_key_shows_notice_and_writes_marker() {
    let home = Sandbox::new();
    let ingress = collector().await;
    let target = home.write_target_file("linked-config.toml", "default_profile = \"default\"\n");
    home.link_config_to(&target);

    let output = home.cx(&endpoint(&ingress)).output().await.unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);
    assert!(String::from_utf8_lossy(&output.stderr).contains(RELEASE_NOTICE));
    assert_notice_marker_wellformed(&home.notice_marker());
}

#[tokio::test]
async fn linked_config_with_telemetry_false_opts_out_without_notice_or_post() {
    let home = Sandbox::new();
    let ingress = collector().await;
    let target = home.write_target_file("linked-config.toml", "telemetry = false\n");
    home.link_config_to(&target);

    let output = home.cx(&endpoint(&ingress)).output().await.unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);
    assert!(!String::from_utf8_lossy(&output.stderr).contains(RELEASE_NOTICE));
    assert!(requests(&ingress).await.is_empty());
}

#[tokio::test]
async fn dangling_config_symlink_opts_out_without_notice_or_post() {
    let home = Sandbox::new();
    let ingress = collector().await;
    let missing_target = home.0.join("missing-config.toml");
    home.link_config_to(&missing_target);

    let output = home.cx(&endpoint(&ingress)).output().await.unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);
    assert!(!String::from_utf8_lossy(&output.stderr).contains(RELEASE_NOTICE));
    assert!(requests(&ingress).await.is_empty());
}

#[tokio::test]
async fn linked_config_with_malformed_toml_opts_out_without_notice_or_post() {
    let home = Sandbox::new();
    let ingress = collector().await;
    let target = home.write_target_file("linked-config.toml", "telemetry = [\n");
    home.link_config_to(&target);

    let output = home.cx(&endpoint(&ingress)).output().await.unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);
    assert!(!String::from_utf8_lossy(&output.stderr).contains(RELEASE_NOTICE));
    assert!(requests(&ingress).await.is_empty());
}

#[tokio::test]
async fn linked_config_with_non_boolean_telemetry_opts_out_without_notice_or_post() {
    let home = Sandbox::new();
    let ingress = collector().await;
    let target = home.write_target_file("linked-config.toml", "telemetry = \"yes\"\n");
    home.link_config_to(&target);

    let output = home.cx(&endpoint(&ingress)).output().await.unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);
    assert!(!String::from_utf8_lossy(&output.stderr).contains(RELEASE_NOTICE));
    assert!(requests(&ingress).await.is_empty());
}
