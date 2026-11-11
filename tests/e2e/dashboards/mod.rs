//! E2E coverage for `cx dashboards` and `cx dashboards folders`.
//!
//! Read-only tests run with any credentials. Write tests (`*_lifecycle`) are
//! gated by `require_write_creds` and only ever touch resources they created
//! themselves under a `cx-e2e-` unique name; they never pick "the first
//! catalog item" for a write.
//!
//! Observed API shapes (eu2, 2026-11):
//! - `create -o json`          -> `{ "dashboardId": "<21-char nanoid>" }`
//! - `folders create -o json`  -> `{ "folderId": "<uuid>" }`
//! - `get -o json`             -> `{ "dashboard": { ... }, "createdAt": ..., ... }`
//! - `replace -o json`         -> `{}` (id is echoed on stderr)
//! - `check -o json`           -> `[]` when valid, `[ {severity, message, location} ]` otherwise
//! - The three Olly KB searches (`search`, `query-search --field|--description`)
//!   return `[]` on the e2e team even for seeded dashboards: the semantic index
//!   is not populated there. Those tests therefore assert shape/exit only, and
//!   the lifecycle polls for a short while before falling back to shape.

use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Pull a string id out of a create response, checking each candidate key in
/// order on the object itself (or on the first element when the CLI emitted
/// an array). The dashboards API uses `dashboardId` / `folderId`, which the
/// generic `harness::id_from_response` does not know about.
fn id_from_keys(v: &Value, keys: &[&str]) -> Option<String> {
    let obj = match v {
        Value::Array(items) => items.first()?,
        other => other,
    };
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(|x| x.as_str()))
        .map(String::from)
}

/// `get -o json` wraps the definition under `dashboard`; older gateways
/// returned it bare. Normalise to the inner object.
fn inner_dashboard(get_resp: &Value) -> Value {
    get_resp
        .get("dashboard")
        .cloned()
        .unwrap_or_else(|| get_resp.clone())
}

/// A minimal but real dashboard: one section, one row, one `dynamic` stat
/// widget that counts otel-demo logs. Having a real DataPrime query gives
/// `check` something to validate and the Olly KB something to index.
fn dashboard_body(name: &str, widget_title: &str) -> Value {
    let section_id = uuid::Uuid::new_v4().to_string();
    let row_id = uuid::Uuid::new_v4().to_string();
    let widget_id = uuid::Uuid::new_v4().to_string();
    let query_id = uuid::Uuid::new_v4().to_string();
    json!({
        "name": name,
        "description": "cx e2e lifecycle dashboard (safe to delete)",
        "layout": {
            "sections": [{
                "id": { "value": section_id },
                "rows": [{
                    "id": { "value": row_id },
                    "appearance": { "height": 19 },
                    "widgets": [{
                        "id": { "value": widget_id },
                        "title": widget_title,
                        "definition": {
                            "dynamic": {
                                "queryDefinitions": [{
                                    "id": query_id,
                                    "query": {
                                        "dataprime": {
                                            "dataprimeQuery": {
                                                "text": "source logs | filter $l.applicationname == 'otel-demo' | aggregate count() as log_count"
                                            },
                                            "dataModeType": "DATA_MODE_TYPE_ARCHIVE"
                                        }
                                    }
                                }],
                                "visualization": {
                                    "stat": {
                                        "valueFields": [{
                                            "keypath": ["log_count"],
                                            "scope": "DATASET_SCOPE_USER_DATA"
                                        }]
                                    }
                                }
                            }
                        }
                    }]
                }],
                "options": {
                    "custom": {
                        "name": "Section",
                        "collapsed": false,
                        "color": { "predefined": "SECTION_PREDEFINED_COLOR_UNSPECIFIED" }
                    }
                }
            }]
        }
    })
}

/// JSON pointer to the first widget's title inside a `get` response.
const FIRST_WIDGET_TITLE: &str = "/dashboard/layout/sections/0/rows/0/widgets/0/title";

/// Discover a dashboard id from `dashboards catalog -o json` (read-only use).
fn discover_dashboard_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["dashboards", "catalog", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}

// ── Read-only ────────────────────────────────────────────────────────────────

#[test]
#[ignore]
fn dashboards_catalog() {
    if harness::require_creds("dashboards_catalog").is_none() {
        return;
    }
    // The e2e team ships seeded `[otel-demo] ...` dashboards, so the catalog
    // is never empty there.
    let v = harness::run_ok_json(&["dashboards", "catalog", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name", "folder"]);
    harness::run_ok_nonempty(&["dashboards", "catalog"]);
    harness::run_ok_nonempty(&["dashboards", "catalog", "-o", "toon"]);
}

#[test]
#[ignore]
fn dashboards_get() {
    if harness::require_creds("dashboards_get").is_none() {
        return;
    }
    let Some(id) = discover_dashboard_id() else {
        eprintln!("[e2e] skipping dashboards_get: no dashboards available on test team");
        return;
    };
    let v = harness::run_ok_json(&["dashboards", "get", &id, "-o", "json"]);
    let dashboard = inner_dashboard(&v);
    harness::assert_object_with_keys(&dashboard, &["id", "name", "layout"]);
    assert_eq!(
        dashboard["id"],
        json!(id),
        "get should echo the requested id"
    );
}

#[test]
#[ignore]
fn dashboards_get_nonexistent_fails() {
    if harness::require_creds("dashboards_get_nonexistent_fails").is_none() {
        return;
    }
    // Ids are 21-char nanoids; the API rejects anything else with 400 and
    // unknown-but-well-formed ids with 404. Both must surface as a failure.
    harness::run_fail(&["dashboards", "get", "nonexistent-id-000", "-o", "json"]);
    harness::run_fail(&["dashboards", "get", "000000000000000000000", "-o", "json"]);
}

#[test]
#[ignore]
fn dashboards_folders_list() {
    if harness::require_creds("dashboards_folders_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["dashboards", "folders", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "parent_id"]);
    harness::run_ok(&["dashboards", "folders", "list"]);
    harness::run_ok(&["dashboards", "folders", "list", "-o", "toon"]);
}

#[test]
#[ignore]
fn dashboards_delete_nonexistent_fails() {
    if harness::require_creds("dashboards_delete_nonexistent_fails").is_none() {
        return;
    }
    let stderr = harness::run_fail(&["dashboards", "delete", "nonexistent-id-000", "--yes"]);
    assert!(
        stderr.contains("Deleting dashboard"),
        "expected 'Deleting dashboard' on stderr, got: {stderr}"
    );
}

#[test]
#[ignore]
fn dashboards_folders_delete_nonexistent_fails() {
    if harness::require_creds("dashboards_folders_delete_nonexistent_fails").is_none() {
        return;
    }
    // Non-uuid -> 400 "invalid uuid"; well-formed unknown uuid -> 404.
    let stderr = harness::run_fail(&[
        "dashboards",
        "folders",
        "delete",
        "nonexistent-id-000",
        "--yes",
    ]);
    assert!(
        stderr.contains("Deleting dashboard folder"),
        "expected 'Deleting dashboard folder' on stderr, got: {stderr}"
    );
    harness::run_fail(&[
        "dashboards",
        "folders",
        "delete",
        "00000000-0000-4000-8000-000000000000",
        "--yes",
    ]);
}

#[test]
#[ignore]
fn dashboards_search_returns_array() {
    if harness::require_creds("dashboards_search").is_none() {
        return;
    }
    // GET /api/v1/olly-kb/dashboards/semantic-search. The e2e team has no
    // semantic index, so only shape and exit status are asserted; the
    // lifecycle test below polls for our own dashboard.
    let v = harness::run_ok_json(&[
        "dashboards",
        "search",
        "request rate and latency",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["dashboard_id", "dashboard_name", "similarity"],
    );
    if harness::assert_array(&v).is_empty() {
        eprintln!("[e2e] dashboards search: no indexed dashboards on test team (shape only)");
    }
    harness::run_ok_nonempty(&["dashboards", "search", "request rate", "--limit", "3"]);
    harness::run_ok_nonempty(&[
        "dashboards",
        "search",
        "request rate",
        "--limit",
        "3",
        "-o",
        "toon",
    ]);
}

#[test]
#[ignore]
fn dashboards_query_search_description_returns_array() {
    if harness::require_creds("dashboards_query_search_description").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "dashboards",
        "query-search",
        "--description",
        "error rate by service",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, &["query_text", "similarity", "dashboard_name"]);
    if harness::assert_array(&v).is_empty() {
        eprintln!(
            "[e2e] dashboards query-search --description: no indexed queries on test team (shape only)"
        );
    }
    harness::run_ok_nonempty(&[
        "dashboards",
        "query-search",
        "--description",
        "error rate",
        "--limit",
        "3",
    ]);
    harness::run_ok_nonempty(&[
        "dashboards",
        "query-search",
        "--description",
        "error rate",
        "--limit",
        "3",
        "-o",
        "toon",
    ]);
}

#[test]
#[ignore]
fn dashboards_query_search_field_returns_array() {
    if harness::require_creds("dashboards_query_search_field").is_none() {
        return;
    }
    // GET /api/v1/olly-kb/queries/by-field.
    let v = harness::run_ok_json(&[
        "dashboards",
        "query-search",
        "--field",
        "$l.applicationname",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["query_text", "dashboard_name", "matched_fields"],
    );
    if harness::assert_array(&v).is_empty() {
        eprintln!(
            "[e2e] dashboards query-search --field: no indexed queries on test team (shape only)"
        );
    }
    harness::run_ok_nonempty(&[
        "dashboards",
        "query-search",
        "--field",
        "$l.applicationname",
        "--limit",
        "3",
    ]);
    harness::run_ok_nonempty(&[
        "dashboards",
        "query-search",
        "--field",
        "$l.applicationname",
        "--limit",
        "3",
        "-o",
        "toon",
    ]);
}

#[test]
#[ignore]
fn dashboards_check_valid_dashboard_exits_ok() {
    if harness::require_creds("dashboards_check_valid_dashboard_exits_ok").is_none() {
        return;
    }
    let Some(id) = discover_dashboard_id() else {
        eprintln!(
            "[e2e] skipping dashboards_check_valid_dashboard_exits_ok: no dashboards available on test team"
        );
        return;
    };
    // POST /mgmt/openapi/5/dashboards/check/v1 with { "dashboardId": <id> }.
    // A stored dashboard must validate cleanly; JSON output is the issue list.
    let v = harness::run_ok_json(&["dashboards", "check", &id, "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["severity"]);
}

#[test]
#[ignore]
fn dashboards_check_nonexistent_fails() {
    if harness::require_creds("dashboards_check_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&["dashboards", "check", "nonexistent-id-000", "-o", "json"]);
}

#[test]
#[ignore]
fn dashboards_check_from_file_round_trip() {
    if harness::require_creds("dashboards_check_from_file_round_trip").is_none() {
        return;
    }
    let Some(id) = discover_dashboard_id() else {
        eprintln!(
            "[e2e] skipping dashboards_check_from_file_round_trip: no dashboards available on test team"
        );
        return;
    };
    // Read-only: fetch a stored definition and validate it inline. The
    // wrapped `get` output is accepted as-is by `--from-file`.
    let original = harness::run_ok_json(&["dashboards", "get", &id, "-o", "json"]);
    let file = harness::JsonFile::new("dash-check", &original);
    let out = harness::run_raw(&[
        "dashboards",
        "check",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Checking dashboard from file"),
        "expected 'Checking dashboard from file' on stderr, got: {stderr}"
    );
    assert!(
        out.status.success(),
        "dashboards check --from-file should exit 0 for a stored dashboard, stderr: {stderr}"
    );
    let issues: Value = serde_json::from_slice(&out.stdout).expect("check emits JSON");
    harness::assert_array_of_objects_with_keys(&issues, &["severity"]);
}

#[test]
#[ignore]
fn dashboards_check_from_file_invalid_definition_fails() {
    if harness::require_creds("dashboards_check_from_file_invalid_definition_fails").is_none() {
        return;
    }
    // Nothing is persisted by `check`, so this is read-only. A non-UUID
    // section id is a SEVERITY_ERROR issue, which must exit non-zero AND
    // still print the issue rows as JSON on stdout.
    let bad = json!({
        "name": harness::unique_name("dash-bad"),
        "layout": { "sections": [{ "id": { "value": "not-a-uuid" }, "rows": [] }] }
    });
    let file = harness::JsonFile::new("dash-bad", &bad);
    let out = harness::run_raw(&[
        "dashboards",
        "check",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert!(
        !out.status.success(),
        "check must fail on an error-severity issue"
    );
    let issues: Value = serde_json::from_slice(&out.stdout).expect("check emits JSON rows");
    harness::assert_nonempty_array_of_objects_with_keys(&issues, &["severity", "message"]);
    assert!(
        issues
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["severity"] == json!("SEVERITY_ERROR")),
        "expected at least one SEVERITY_ERROR issue: {issues}"
    );
    // Text mode must fail the same way.
    harness::run_fail(&["dashboards", "check", "--from-file", file.path()]);
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

#[test]
#[ignore]
fn dashboards_folders_lifecycle() {
    if harness::require_write_creds("dashboards_folders_lifecycle").is_none() {
        return;
    }
    let parent_name = harness::unique_name("dfolder");
    let child_name = harness::unique_name("dfolder-child");

    // create (top-level)
    let created = harness::run_write_json(&[
        "dashboards",
        "folders",
        "create",
        "--name",
        &parent_name,
        "-o",
        "json",
    ]);
    let parent_id = id_from_keys(&created, &["folderId", "id"])
        .unwrap_or_else(|| panic!("folder create returned no folderId: {created}"));
    let mut parent_cleanup =
        harness::Cleanup::new(&["dashboards", "folders", "delete", &parent_id]);

    // list contains it, at the root
    let list = harness::run_ok_json(&["dashboards", "folders", "list", "-o", "json"]);
    let row = harness::find_by_name(&list, "name", &parent_name)
        .unwrap_or_else(|| panic!("folder list should contain {parent_name}: {list}"));
    assert_eq!(row["id"], json!(parent_id));
    assert!(
        row["parent_id"].is_null(),
        "top-level folder has no parent: {row}"
    );

    // create (nested via --parent-id)
    let created = harness::run_write_json(&[
        "dashboards",
        "folders",
        "create",
        "--name",
        &child_name,
        "--parent-id",
        &parent_id,
        "-o",
        "json",
    ]);
    let child_id = id_from_keys(&created, &["folderId", "id"])
        .unwrap_or_else(|| panic!("nested folder create returned no folderId: {created}"));
    let mut child_cleanup = harness::Cleanup::new(&["dashboards", "folders", "delete", &child_id]);

    let list = harness::run_ok_json(&["dashboards", "folders", "list", "-o", "json"]);
    let row = harness::find_by_name(&list, "name", &child_name)
        .unwrap_or_else(|| panic!("folder list should contain {child_name}: {list}"));
    assert_eq!(row["id"], json!(child_id));
    assert_eq!(
        row["parent_id"],
        json!(parent_id),
        "nested folder must point at its parent: {row}"
    );

    // text + toon renderers show the created folders
    let text = String::from_utf8_lossy(&harness::run_ok_nonempty(&[
        "dashboards",
        "folders",
        "list",
    ]))
    .into_owned();
    assert!(text.contains(&parent_name) && text.contains(&child_name));
    let toon = String::from_utf8_lossy(&harness::run_ok_nonempty(&[
        "dashboards",
        "folders",
        "list",
        "-o",
        "toon",
    ]))
    .into_owned();
    assert!(toon.contains(&child_name));

    // text-mode create must also print the id on stderr
    let extra_name = harness::unique_name("dfolder-text");
    let out = harness::run_raw(&[
        "dashboards",
        "folders",
        "create",
        "--name",
        &extra_name,
        "--parent-id",
        &parent_id,
        "--yes",
    ]);
    assert!(out.status.success(), "text-mode folder create failed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Created dashboard folder") && stderr.contains("(ID: "),
        "text-mode create should report the new id on stderr: {stderr}"
    );
    let list = harness::run_ok_json(&["dashboards", "folders", "list", "-o", "json"]);
    let extra_id = harness::find_by_name(&list, "name", &extra_name)
        .and_then(|r| r["id"].as_str().map(String::from))
        .unwrap_or_else(|| panic!("folder list should contain {extra_name}: {list}"));
    harness::run_write_ok(&["dashboards", "folders", "delete", &extra_id]);

    // delete child, then parent; a second delete must fail; list no longer has them
    harness::run_write_ok(&["dashboards", "folders", "delete", &child_id]);
    child_cleanup.disarm();
    harness::run_fail(&["dashboards", "folders", "delete", &child_id, "--yes"]);
    harness::run_write_ok(&["dashboards", "folders", "delete", &parent_id]);
    parent_cleanup.disarm();
    harness::run_fail(&["dashboards", "folders", "delete", &parent_id, "--yes"]);

    let list = harness::run_ok_json(&["dashboards", "folders", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "name", &parent_name).is_none()
            && harness::find_by_name(&list, "name", &child_name).is_none()
            && harness::find_by_name(&list, "name", &extra_name).is_none(),
        "deleted folders must disappear from the list: {list}"
    );
}

#[test]
#[ignore]
fn dashboards_lifecycle() {
    if harness::require_write_creds("dashboards_lifecycle").is_none() {
        return;
    }
    let folder_name = harness::unique_name("dfolder-dash");
    let name = harness::unique_name("dash");
    let widget_title = format!("{name} total logs");

    // a folder of our own so `create --folder` can be asserted end to end
    let created = harness::run_write_json(&[
        "dashboards",
        "folders",
        "create",
        "--name",
        &folder_name,
        "-o",
        "json",
    ]);
    let folder_id = id_from_keys(&created, &["folderId", "id"])
        .unwrap_or_else(|| panic!("folder create returned no folderId: {created}"));
    let mut folder_cleanup =
        harness::Cleanup::new(&["dashboards", "folders", "delete", &folder_id]);

    // create --from-file --folder
    let body = dashboard_body(&name, &widget_title);
    let file = harness::JsonFile::new("dash", &body);
    let created = harness::run_write_json(&[
        "dashboards",
        "create",
        "--from-file",
        file.path(),
        "--folder",
        &folder_id,
        "-o",
        "json",
    ]);
    let id = id_from_keys(&created, &["dashboardId", "id"])
        .unwrap_or_else(|| panic!("dashboard create returned no dashboardId: {created}"));
    assert_eq!(id.len(), 21, "dashboard ids are 21-char nanoids: {id}");
    let mut cleanup = harness::Cleanup::new(&["dashboards", "delete", &id]);

    // get: our name, our folder, our widget
    let got = harness::run_ok_json(&["dashboards", "get", &id, "-o", "json"]);
    let dashboard = inner_dashboard(&got);
    harness::assert_object_with_keys(&dashboard, &["id", "name", "layout"]);
    assert_eq!(dashboard["id"], json!(id));
    assert_eq!(dashboard["name"], json!(name));
    assert_eq!(
        dashboard.pointer("/folderId/value"),
        Some(&json!(folder_id)),
        "create --folder should land the dashboard in our folder: {dashboard}"
    );
    assert_eq!(
        got.pointer(FIRST_WIDGET_TITLE),
        Some(&json!(widget_title)),
        "widget should round-trip: {got}"
    );

    // catalog contains it (by name) with the folder resolved
    let catalog = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["dashboards", "catalog", "-o", "json"]);
        harness::find_by_name(&list, "name", &name).cloned()
    })
    .unwrap_or_else(|| panic!("catalog should contain {name}"));
    assert_eq!(catalog["id"], json!(id));
    assert_eq!(catalog["folder"]["id"], json!(folder_id));
    assert_eq!(catalog["folder"]["name"], json!(folder_name));

    // check <id>: valid, no issues (json + text)
    let issues = harness::run_ok_json(&["dashboards", "check", &id, "-o", "json"]);
    assert_eq!(
        issues,
        json!([]),
        "a freshly created dashboard must have no issues"
    );
    let text = String::from_utf8_lossy(&harness::run_ok_nonempty(&["dashboards", "check", &id]))
        .into_owned();
    assert!(
        text.contains("Dashboard is valid"),
        "text check output: {text}"
    );

    // check --from-file: the wrapped `get` output validates as-is
    let got_file = harness::JsonFile::new("dash-get", &got);
    let issues = harness::run_ok_json(&[
        "dashboards",
        "check",
        "--from-file",
        got_file.path(),
        "-o",
        "json",
    ]);
    assert_eq!(issues, json!([]));

    // replace --from-file: rename + retitle the widget using the wrapped get output
    let renamed = format!("{name}-renamed");
    let retitled = format!("{widget_title} renamed");
    let mut edited = got.clone();
    edited["dashboard"]["name"] = json!(renamed);
    *edited
        .pointer_mut(FIRST_WIDGET_TITLE)
        .expect("widget title path") = json!(retitled);
    let upd = harness::JsonFile::new("dash-upd", &edited);
    let out = harness::run_raw(&[
        "dashboards",
        "replace",
        "--from-file",
        upd.path(),
        "-o",
        "json",
        "--yes",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "replace should succeed, stderr: {stderr}"
    );
    assert!(
        stderr.contains("Replacing dashboard") && stderr.contains(&id),
        "replace should report the id on stderr: {stderr}"
    );
    let got = harness::run_ok_json(&["dashboards", "get", &id, "-o", "json"]);
    assert_eq!(
        got["dashboard"]["name"],
        json!(renamed),
        "replace should rename: {got}"
    );
    assert_eq!(
        got.pointer(FIRST_WIDGET_TITLE),
        Some(&json!(retitled)),
        "replace should update the widget: {got}"
    );
    assert_eq!(
        got.pointer("/dashboard/folderId/value"),
        Some(&json!(folder_id)),
        "replace must keep the folder assignment: {got}"
    );

    // replace again with the bare (unwrapped) definition, text + toon renderers
    let bare = inner_dashboard(&got);
    let bare_file = harness::JsonFile::new("dash-bare", &bare);
    harness::run_write_ok(&["dashboards", "replace", "--from-file", bare_file.path()]);
    harness::run_write_ok(&[
        "dashboards",
        "replace",
        "--from-file",
        bare_file.path(),
        "-o",
        "toon",
    ]);

    // text + toon renderers on the created dashboard
    let text = String::from_utf8_lossy(&harness::run_ok_nonempty(&["dashboards", "get", &id]))
        .into_owned();
    assert!(
        text.contains(&renamed) && text.contains(&id),
        "text get: {text}"
    );
    let toon = String::from_utf8_lossy(&harness::run_ok_nonempty(&[
        "dashboards",
        "get",
        &id,
        "-o",
        "toon",
    ]))
    .into_owned();
    assert!(toon.contains(&id), "toon get: {toon}");

    // Olly KB semantic search over our own dashboard. The index on the e2e
    // team has not been observed to populate (even seeded dashboards return
    // nothing), so poll briefly and otherwise settle for shape + exit 0.
    let found = harness::eventually(6, Duration::from_secs(10), || {
        let v = harness::run_ok_json(&[
            "dashboards",
            "search",
            &renamed,
            "--limit",
            "5",
            "-o",
            "json",
        ]);
        harness::assert_array_of_objects_with_keys(&v, &["dashboard_id", "similarity"]);
        v.as_array()?
            .iter()
            .find(|r| r["dashboard_id"] == json!(id))
            .cloned()
    });
    match found {
        Some(hit) => assert_eq!(hit["dashboard_name"], json!(renamed)),
        None => eprintln!(
            "[e2e] dashboards search did not index {renamed} within ~60s; asserting shape only"
        ),
    }
    let by_field = harness::run_ok_json(&[
        "dashboards",
        "query-search",
        "--field",
        "$l.applicationname",
        "--limit",
        "10",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&by_field, &["query_text", "matched_fields"]);
    let by_desc = harness::run_ok_json(&[
        "dashboards",
        "query-search",
        "--description",
        "count of otel-demo logs",
        "--limit",
        "10",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&by_desc, &["query_text", "similarity"]);

    // delete, then get / check / delete must fail; catalog no longer lists it
    harness::run_write_ok(&["dashboards", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["dashboards", "get", &id, "-o", "json"]);
    harness::run_fail(&["dashboards", "check", &id, "-o", "json"]);
    harness::run_fail(&["dashboards", "delete", &id, "--yes"]);
    let gone = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["dashboards", "catalog", "-o", "json"]);
        harness::find_by_name(&list, "name", &renamed)
            .is_none()
            .then_some(())
    });
    assert!(gone.is_some(), "deleted dashboard must leave the catalog");

    // folder last (it is empty now)
    harness::run_write_ok(&["dashboards", "folders", "delete", &folder_id]);
    folder_cleanup.disarm();
    let list = harness::run_ok_json(&["dashboards", "folders", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "name", &folder_name).is_none(),
        "deleted folder must leave the list: {list}"
    );
}

#[test]
#[ignore]
fn dashboards_create_rejects_missing_layout() {
    if harness::require_write_creds("dashboards_create_rejects_missing_layout").is_none() {
        return;
    }
    // The CLI validates `layout` before any API call, so nothing is created.
    let file = harness::JsonFile::new(
        "dash-nolayout",
        &json!({ "name": harness::unique_name("dash-nolayout") }),
    );
    let stderr = harness::run_fail(&[
        "dashboards",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
        "--yes",
    ]);
    assert!(
        stderr.contains("missing required 'layout'"),
        "expected layout validation error, got: {stderr}"
    );
}

#[test]
#[ignore]
fn dashboards_replace_rejects_missing_id() {
    if harness::require_write_creds("dashboards_replace_rejects_missing_id").is_none() {
        return;
    }
    // `replace` needs the dashboard id in the body; without it the CLI must
    // refuse before calling the API.
    let body = dashboard_body(&harness::unique_name("dash-noid"), "widget");
    let file = harness::JsonFile::new("dash-noid", &body);
    let stderr = harness::run_fail(&[
        "dashboards",
        "replace",
        "--from-file",
        file.path(),
        "-o",
        "json",
        "--yes",
    ]);
    assert!(
        stderr.contains("missing required 'id'"),
        "expected id validation error, got: {stderr}"
    );
}
