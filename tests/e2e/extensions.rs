use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

/// Small catalog extensions (a handful of alerts/dashboards each) that the
/// lifecycle test may deploy. Extensions are team-global and cannot be
/// namespaced, so the test deploys only ONE item of the first candidate that
/// is not deployed yet and undeploys it again. Two overlapping runs pick
/// different candidates unless they start within the same second (then the
/// second run moves to the next candidate on the deploy conflict).
///
/// Every deployment is scoped, from the very first `deploy` call, to a
/// run-unique application name (`cx-e2e-ext-...`). A run that is killed
/// after `deploy` therefore leaves a deployment whose `applications` column
/// in `extensions deployed` names the run that owns it; the sweeper and a
/// human can tell it apart from a real deployment and remove it.
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
    let deployed_now =
        harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
    let deployed_ids_now: Vec<&str> = deployed_now
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e.get("id").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();

    // The application scope is run-unique from the FIRST deploy call, so a
    // leftover from a killed run is identifiable in `extensions deployed`.
    let app = harness::unique_name("ext");

    // Pick the first candidate that is not deployed, deploy one item of its
    // latest revision; on a deploy conflict (parallel run) try the next one.
    let mut chosen: Option<(String, String, String)> = None;
    for id in CANDIDATES {
        if deployed_ids_now.contains(id) {
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
            &json!({
                "id": id,
                "version": version,
                "itemIds": [item_id],
                "applications": [app],
            }),
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
    // Every candidate taken is a real failure, not a skip: either a killed
    // run left its deployment behind (its `applications` entry starts with
    // `cx-e2e-ext-` and names the run) or someone deployed the candidates on
    // the test team by hand. Skipping here would hide a leak forever.
    let Some((id, version, item_id)) = chosen else {
        let taken: Vec<String> = deployed_now
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| {
                e.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|d| CANDIDATES.contains(&d))
            })
            .map(|e| {
                format!(
                    "{} (applications: {})",
                    e["id"].as_str().unwrap_or("?"),
                    e["applications"]
                )
            })
            .collect();
        panic!(
            "extensions_lifecycle: every candidate extension {CANDIDATES:?} is already \
             deployed on the test team, so there is nothing left to deploy. Deployed \
             candidates: {taken:?}. An entry whose applications start with `cx-e2e-ext-` \
             is a leftover of a killed e2e run: undeploy it with \
             `cx integrations extensions undeploy --from-file <{{\"id\": \"<id>\"}}> --yes`."
        );
    };
    let undeploy_file = harness::JsonFile::new("ext-undeploy", &json!({ "id": id }));
    let mut cleanup = harness::Cleanup::new(&[
        "integrations",
        "extensions",
        "undeploy",
        "--from-file",
        undeploy_file.path(),
    ]);

    // deployed lists it with the version we asked for, scoped to our app
    let entry = harness::eventually(10, Duration::from_secs(2), || {
        let v = harness::run_ok_json(&["integrations", "extensions", "deployed", "-o", "json"]);
        harness::find_by_name(&v, "id", &id).cloned()
    })
    .unwrap_or_else(|| panic!("deployed should list {id}"));
    assert_eq!(entry["version"], version, "deployed version: {entry}");
    assert_eq!(
        entry["applications"],
        json!([app]),
        "deploy should scope {id} to application {app}: {entry}"
    );

    // update: add a second run-unique application to the scope
    let app2 = harness::unique_name("ext-upd");
    let upd = harness::JsonFile::new(
        "ext-upd",
        &json!({
            "id": id,
            "version": version,
            "itemIds": [item_id],
            "applications": [app, app2],
        }),
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
                    .is_some_and(|a| a.contains(&json!(app)) && a.contains(&json!(app2)))
            })
            .cloned()
    })
    .unwrap_or_else(|| panic!("update should scope {id} to applications {app} and {app2}"));
    assert_eq!(entry["version"], version);

    // text + toon renderers while the deployment exists; both show our scope
    let text = String::from_utf8_lossy(&harness::run_ok_nonempty(&[
        "integrations",
        "extensions",
        "deployed",
    ]))
    .into_owned();
    assert!(text.contains(&app), "text output should show {app}: {text}");
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
