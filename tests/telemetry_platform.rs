#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value::Value, KeyValue};
use prost::Message;
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TRACE_PATH: &str = "/v1/traces";

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("cx-telemetry-platform-{}", uuid::Uuid::new_v4())))
    }

    fn cx(&self, args: &[&str], endpoint: &str, path_value: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
        command
            .env_clear()
            .env("CX_HOME", &self.0)
            .env("CX_TELEMETRY_ENDPOINT", endpoint)
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .env("CX_NO_SKILLS_AUTO_UPDATE", "1")
            .env("PATH", path_value)
            .args(args);
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

fn endpoint(server: &MockServer) -> String {
    format!("{}{TRACE_PATH}", server.uri())
}

fn decode(request: &Request) -> ExportTraceServiceRequest {
    assert_eq!(request.url.path(), TRACE_PATH);
    assert_eq!(request.headers["content-type"], "application/x-protobuf");
    ExportTraceServiceRequest::decode(request.body.as_slice()).unwrap()
}

fn text<'a>(attrs: &'a [KeyValue], name: &str) -> Option<&'a str> {
    attrs.iter().find_map(|attribute| {
        (attribute.key == name)
            .then_some(attribute.value.as_ref()?.value.as_ref()?)
            .and_then(|value| match value {
                Value::StringValue(text) => Some(text.as_str()),
                _ => None,
            })
    })
}

fn write_helper_script(directory: &Path, name: &str, output: &str) {
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' '{name}' >> \"$CX_PLATFORM_HELPER_MARKER\"\nprintf '%s\\n' \"{output}\"\n"
    );
    fs::write(directory.join(name), script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join(name);
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

#[tokio::test]
async fn unix_telemetry_never_spawns_os_helpers_for_platform_metadata() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;

    let sandbox = Sandbox::new();
    let fake_bin = sandbox.0.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let marker = sandbox.0.join("helper-marker.log");
    write_helper_script(&fake_bin, "uname", "6.8.0-52-generic");
    write_helper_script(&fake_bin, "lsb_release", "Release:\t24.04");
    write_helper_script(&fake_bin, "sw_vers", "ProductVersion:\t14.7.1");

    let preflight = Command::new(fake_bin.join("uname"))
        .env_clear()
        .env("CX_PLATFORM_HELPER_MARKER", &marker)
        .output()
        .await
        .unwrap();
    assert!(preflight.status.success());
    assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "uname");
    fs::remove_file(&marker).unwrap();

    let output = sandbox
        .cx(&["--version"], &endpoint(&collector), &fake_bin)
        .env("CX_PLATFORM_HELPER_MARKER", &marker)
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "cx exited with {}", output.status);

    let received = collector.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    assert!(
        !marker.exists() || fs::read_to_string(&marker).unwrap().trim().is_empty(),
        "telemetry should not spawn unix helper binaries"
    );

    let export = decode(&received[0]);
    let resource = &export.resource_spans[0]
        .resource
        .as_ref()
        .unwrap()
        .attributes;
    if let Some(version) = text(resource, "os.version") {
        assert!(version.len() <= 32);
        assert!(version
            .split('.')
            .all(|part| { !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) }));
    }
    if let Some(arch) = text(resource, "host.arch") {
        assert!(matches!(
            arch,
            "amd64" | "arm64" | "x86" | "arm32" | "ppc64" | "riscv64" | "s390x"
        ));
    }
}
