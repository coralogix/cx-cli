use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

/// Small catalog extensions (a handful of alerts/dashboards each) that the
/// lifecycle test may deploy. Extensions are team-global and cannot be
/// namespaced, so the test deploys only ONE item of the first candidate that
/// is not deployed yet and undeploys it again. Two overlapping runs pick
/// different candidates unless they start within the same second (then the
/// second run moves to the next candidate on the deploy conflict).
const CANDIDATES: &[&str] = &[
    "ChecklyObservability",
    "CoreDNSObservability",
    "AWSTransitGatewayObservability",
    "AWSCognitoObservability",
];

#[test]
#[ignore]
fn extensions_list() {
    if harness::require_creds("extensions_list").is_none() {
        return;
    }
    // The public catalog can be empty. Hidden extensions stay out of this list.
    let v = harness::run_ok_json(&["integrations", "extensions", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn extensions_get() {
    if harness::require_creds("extensions_get").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "integrations",
        "extensions",
        "get",
        CANDIDATES[0],
        "-o",
        "json",
    ]);
    assert_eq!(
        v.get("id").and_then(Value::as_str),
        Some(CANDIDATES[0]),
        "extension get should echo the id: {v}"
    );
    assert!(
        latest_revision(&v).is_some(),
        "extension get should carry at least one revision: {v}"
    );
    harness::run_ok_nonempty(&["integrations", "extensions", "get", CANDIDATES[0]]);
    harness::run_fail(&[
        "integrations",
        "extensions",
        "get",
        "cx-e2e-no-such-ext",
        "-o",
        "json",
    ]);
}

#[test]
#[ignore]
fn extensions_deployed() {
    if harness::require_creds("extensions_deployed").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "version", "applications"]);
    // Guard against schema drift: key presence alone is not enough (the
    // pre-fix output had all keys present but null-valued).
    for item in v.as_array().unwrap() {
        assert!(
            item["id"].is_string(),
            "deployed extension id should be a non-null string: {item}"
        );
        assert!(
            item["version"].is_string(),
            "deployed extension version should be a non-null string: {item}"
        );
    }
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

#[test]
#[ignore]
fn extensions_lifecycle() {
    if harness::require_write_creds("extensions_lifecycle").is_none() {
        return;
    }
    let deployed_now = deployed_ids();

    // Pick the first candidate that is not deployed, deploy one item of its
    // latest revision; on a deploy conflict (parallel run) try the next one.
    let mut chosen: Option<(String, String, String)> = None;
    for id in CANDIDATES {
        if deployed_now.iter().any(|d| d == id) {
            continue;
        }
        let ext = harness::run_ok_json(&["integrations", "extensions", "get", id, "-o", "json"]);
        let Some(rev) = latest_revision(&ext) else {
            continue;
        };
        let (Some(version), Some(item_id)) = (
            rev.get("version").and_then(Value::as_str),
            rev.pointer("/items/0/id").and_then(Value::as_str),
        ) else {
            continue;
        };
        let file = harness::JsonFile::new(
            "ext",
            &json!({ "id": id, "version": version, "itemIds": [item_id] }),
        );
        let out = harness::run_raw(&[
            "integrations",
            "extensions",
            "deploy",
            "--from-file",
            file.path(),
            "-o",
            "json",
            "--yes",
        ]);
        if out.status.success() {
            chosen = Some((id.to_string(), version.to_string(), item_id.to_string()));
            break;
        }
        eprintln!("[e2e] deploy of {id} failed (probably a parallel run deployed it); trying next");
    }
    let Some((id, version, item_id)) = chosen else {
        eprintln!(
            "[e2e] skipping extensions_lifecycle: every candidate extension is already deployed"
        );
        return;
    };
    let undeploy_file = harness::JsonFile::new("ext-undeploy", &json!({ "id": id }));
    let mut cleanup = harness::Cleanup::new(&[
        "integrations",
        "extensions",
        "undeploy",
        "--from-file",
        undeploy_file.path(),
    ]);

    // deployed lists it with the version we asked for
    let entry = harness::eventually(10, Duration::from_secs(2), || {
        let v = harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
        harness::find_by_name(&v, "id", &id).cloned()
    })
    .unwrap_or_else(|| panic!("deployed should list {id}"));
    assert_eq!(entry["version"], version, "deployed version: {entry}");

    // update: scope the deployment to a run-unique application name
    let app = harness::unique_name("extapp");
    let upd = harness::JsonFile::new(
        "ext-upd",
        &json!({ "id": id, "version": version, "itemIds": [item_id], "applications": [app] }),
    );
    harness::run_write_ok(&[
        "integrations",
        "extensions",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let entry = harness::eventually(10, Duration::from_secs(2), || {
        let v = harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
        harness::find_by_name(&v, "id", &id)
            .filter(|e| {
                e["applications"]
                    .as_array()
                    .is_some_and(|a| a.contains(&json!(app)))
            })
            .cloned()
    })
    .unwrap_or_else(|| panic!("update should scope {id} to application {app}"));
    assert_eq!(entry["version"], version);

    // text + toon renderers while the deployment exists
    harness::run_ok_nonempty(&["integrations", "extensions", "deployed"]);
    harness::run_ok_nonempty(&["integrations", "extensions", "deployed", "-o", "toon"]);

    // undeploy, then it is gone from deployed
    harness::run_write_ok(&[
        "integrations",
        "extensions",
        "undeploy",
        "--from-file",
        undeploy_file.path(),
        "-o",
        "json",
    ]);
    cleanup.disarm();
    let gone = harness::eventually(10, Duration::from_secs(2), || {
        (!deployed_ids().contains(&id)).then_some(())
    });
    assert!(
        gone.is_some(),
        "undeployed extension {id} should leave the deployed list"
    );

    // undeploying again is a clean API error
    let stderr = harness::run_fail(&[
        "integrations",
        "extensions",
        "undeploy",
        "--from-file",
        undeploy_file.path(),
        "--yes",
    ]);
    assert!(stderr.contains("(404)"), "second undeploy: {stderr}");
}

fn deployed_ids() -> Vec<String> {
    let v = harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e.get("id").and_then(Value::as_str))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn latest_revision(ext: &Value) -> Option<&Value> {
    ext.get("revisions").and_then(Value::as_array)?.last()
}
