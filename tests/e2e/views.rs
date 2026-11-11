use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn views_list() {
    if harness::require_creds("views_list").is_none() {
        return;
    }
    // The writable e2e team starts with no views; lifecycle tests below
    // create their own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["views", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn views_get() {
    if harness::require_creds("views_get").is_none() {
        return;
    }
    let id = match discover_view_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping views_get: no views available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["views", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn views_folders_list() {
    if harness::require_creds("views_folders_list").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["views", "folders", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn views_folders_get() {
    if harness::require_creds("views_folders_get").is_none() {
        return;
    }
    let id = match discover_view_folder_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping views_folders_get: no view folders available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["views", "folders", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

fn discover_view_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("views_discover")?;
            let stdout = harness::run_ok(&["views", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}

fn discover_view_folder_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("views_folders_discover")?;
            let stdout = harness::run_ok(&["views", "folders", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

#[test]
#[ignore]
fn views_folders_lifecycle() {
    if harness::require_write_creds("views_folders_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("vfolder");

    // create
    let file = harness::JsonFile::new("vfolder", &json!({ "name": name }));
    let created = harness::run_write_json(&[
        "views",
        "folders",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("folder create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["views", "folders", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["views", "folders", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name"]);
    assert_eq!(got["name"], name, "folder get should return our name");

    // list contains it
    let list = harness::run_ok_json(&["views", "folders", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "name", &name).is_some(),
        "folder list should contain {name}"
    );

    // update
    let renamed = format!("{name}-renamed");
    let upd = harness::JsonFile::new("vfolder-upd", &json!({ "id": id, "name": renamed }));
    harness::run_write_ok(&[
        "views",
        "folders",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["views", "folders", "get", &id, "-o", "json"]);
    assert_eq!(got["name"], renamed, "folder update should rename");

    // delete, then get must fail
    harness::run_write_ok(&["views", "folders", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["views", "folders", "get", &id, "-o", "json"]);
}

#[test]
#[ignore]
fn views_lifecycle() {
    if harness::require_write_creds("views_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("view");
    let body = json!({
        "name": name,
        "search_query": { "query": "error" },
        "time_selection": { "quick_selection": { "caption": "Last 15 minutes", "seconds": 900 } },
        "filters": { "filters": [] }
    });

    // create
    let file = harness::JsonFile::new("view", &body);
    let created =
        harness::run_write_json(&["views", "create", "--from-file", file.path(), "-o", "json"]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("view create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["views", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["views", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name"]);
    assert_eq!(got["name"], name);

    // list contains it
    let list = harness::run_ok_json(&["views", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "name", &name).is_some(),
        "view list should contain {name}"
    );

    // update: change the query
    let mut updated = body.clone();
    updated["id"] = json!(id);
    updated["search_query"]["query"] = json!("warning");
    let upd = harness::JsonFile::new("view-upd", &updated);
    harness::run_write_ok(&[
        "views",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["views", "get", &id, "-o", "json"]);
    let q = got
        .pointer("/searchQuery/query")
        .or_else(|| got.pointer("/search_query/query"))
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        q,
        json!("warning"),
        "view update should change the query: {got}"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["views", "get", &id]);
    harness::run_ok_nonempty(&["views", "get", &id, "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["views", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["views", "get", &id, "-o", "json"]);
}
