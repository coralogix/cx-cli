//! `cx rum`: source-map releases and stack-trace mapping.
//!
//! The test team has no uploaded source maps, so `stacktrace` can only be
//! driven against a release that does not exist: the API answers 404 and cx
//! must surface that message. When a successfully uploaded web release is
//! present, the happy path runs too.

use crate::harness;

const NO_SUCH_APP: &str = "cx-e2e-no-such-app";

#[test]
#[ignore]
fn rum_source_maps_list() {
    if harness::require_creds("rum_source_maps_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["rum", "source-maps", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["application", "version", "platform"]);
    if v.as_array().unwrap().is_empty() {
        eprintln!("[e2e] rum source-maps list: no source maps uploaded on test team");
    }
}

/// `--application` narrows the list; a name nobody uploaded for yields an
/// empty list, not an error. Text mode says so in words.
#[test]
#[ignore]
fn rum_source_maps_list_filtered_by_application() {
    if harness::require_creds("rum_source_maps_list_filtered_by_application").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "rum",
        "source-maps",
        "list",
        "--application",
        NO_SUCH_APP,
        "-o",
        "json",
    ]);
    assert_eq!(v, serde_json::json!([]), "{v}");
    let text =
        harness::run_ok_nonempty(&["rum", "source-maps", "list", "--application", NO_SUCH_APP]);
    assert!(String::from_utf8_lossy(&text).contains("No source map releases found"));
    harness::run_ok(&["rum", "source-maps", "list", "-o", "toon"]);
}

fn stacktrace_output(args: &[&str]) -> std::process::Output {
    harness::cx()
        .args([
            "rum",
            "stacktrace",
            "--application",
            NO_SUCH_APP,
            "--version",
            "0.0.1",
        ])
        .args(args)
        .output()
        .expect("cx must run")
}

/// `--frame` and `--platform` reach the API; a missing release is a clear 404.
#[test]
#[ignore]
fn rum_stacktrace_frame_flag_reports_missing_release() {
    if harness::require_creds("rum_stacktrace_frame_flag_reports_missing_release").is_none() {
        return;
    }
    let output = stacktrace_output(&[
        "--platform",
        "web",
        "--frame",
        "https://example.com/main.js:1:1",
        "--frame",
        "https://example.com/vendor.js:12",
        "-o",
        "json",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Mapping 2 frames"), "{stderr}");
    assert!(
        stderr.contains("No source maps were uploaded") && stderr.contains(NO_SUCH_APP),
        "{stderr}"
    );
}

/// `--from-file` reads RUM's original_stacktrace JSON; frames without a line
/// are skipped with a notice, and the android platform alias is accepted.
#[test]
#[ignore]
fn rum_stacktrace_from_file_and_platform_alias() {
    if harness::require_creds("rum_stacktrace_from_file_and_platform_alias").is_none() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("cx_e2e_rum_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let frames = dir.join("frames.json");
    std::fs::write(
        &frames,
        r#"[{"fileName":"https://example.com/main.js","lineNumber":1,"columnNumber":1},
            {"fileName":"native","lineNumber":null}]"#,
    )
    .unwrap();
    let output = stacktrace_output(&[
        "--platform",
        "android",
        "--from-file",
        frames.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Skipped 1 frames without a line number"),
        "{stderr}"
    );
    assert!(stderr.contains("Mapping 1 frames"), "{stderr}");
    assert!(stderr.contains("No source maps were uploaded"), "{stderr}");

    // Local validation happens before any request.
    let output = stacktrace_output(&["--platform", "windows", "--frame", "a.js:1"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown platform"));
    let output = stacktrace_output(&["--frame", "not-a-frame"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid frame"));
}

// Maps one frame of the newest web release. The frame may not resolve, so this checks the shape, not the mapping.
#[test]
#[ignore]
fn rum_stacktrace() {
    if harness::require_creds("rum_stacktrace").is_none() {
        return;
    }
    let releases = harness::run_ok_json(&["rum", "source-maps", "list", "-o", "json"]);
    let Some(release) = harness::assert_array(&releases).iter().find(|r| {
        r["platform"] == "SOURCE_MAP_PLATFORM_WEB"
            && r["uploadStatus"] == "SOURCE_MAP_UPLOAD_STATUS_SUCCEEDED"
    }) else {
        eprintln!(
            "[e2e] skipping rum_stacktrace: no successfully uploaded web source maps on test team"
        );
        return;
    };
    let (app, version) = (
        release["application"].as_str().unwrap(),
        release["version"].as_str().unwrap(),
    );
    let v = harness::run_ok_json(&[
        "rum",
        "stacktrace",
        "--application",
        app,
        "--version",
        version,
        "--platform",
        "web",
        "--frame",
        "https://example.com/main.js:1:1",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["application", "version", "frames"]);
}
