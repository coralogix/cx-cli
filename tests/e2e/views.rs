use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn views_list() {
    if harness::require_creds("views_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["views", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
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
            if harness::require_creds("views_discover").is_none() {
                return None;
            }
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
            if harness::require_creds("views_folders_discover").is_none() {
                return None;
            }
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
