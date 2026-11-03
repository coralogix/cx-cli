use crate::harness;

#[test]
#[ignore]
fn rum_source_maps_list() {
    if harness::require_creds("rum_source_maps_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["rum", "source-maps", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["application", "version", "platform"]);
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
        "--frame",
        "https://example.com/main.js:1:1",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["application", "version", "frames"]);
}
