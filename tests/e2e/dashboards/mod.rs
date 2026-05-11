use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn dashboards_catalog() {
    if harness::require_creds("dashboards_catalog").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["dashboards", "catalog", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
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
    // Shape varies (top-level vs nested under "dashboard") - only assert object.
    harness::assert_get_response(&v, &[]);
}

/// Discover a dashboard id from `dashboards catalog -o json`.
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

#[test]
#[ignore]
fn dashboards_delete_nonexistent() {
    if harness::require_creds("dashboards_delete_nonexistent").is_none() {
        return;
    }
    // Attempting to delete a non-existent dashboard - the CLI should at least
    // parse the command and attempt the API call (we don't assert success since
    // the resource won't exist).
    let output = harness::cx()
        .args(["dashboards", "delete", "nonexistent-id-000", "--yes"])
        .output()
        .expect("failed to execute cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Verify the CLI parsed the command (shows the "Deleting..." status line).
    assert!(
        stderr.contains("Deleting dashboard"),
        "expected 'Deleting dashboard' on stderr, got: {stderr}"
    );
}

#[test]
#[ignore]
fn dashboards_folders_delete_nonexistent() {
    if harness::require_creds("dashboards_folders_delete_nonexistent").is_none() {
        return;
    }
    let output = harness::cx()
        .args([
            "dashboards",
            "folders",
            "delete",
            "nonexistent-id-000",
            "--yes",
        ])
        .output()
        .expect("failed to execute cx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Deleting dashboard folder"),
        "expected 'Deleting dashboard folder' on stderr, got: {stderr}"
    );
}

#[test]
#[ignore]
fn dashboards_query_search_description_returns_results() {
    if harness::require_creds("dashboards_query_search_description").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "dashboards",
        "query-search",
        "--description",
        "data usage by application",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    let arr = v.as_array().expect("should be a JSON array");
    assert!(
        !arr.is_empty(),
        "query-search --description should return at least one result for this account"
    );
    harness::assert_array_of_objects_with_keys(&v, &["query_text", "similarity", "dashboard_name"]);
}

#[test]
#[ignore]
fn dashboards_query_search_field_returns_results() {
    if harness::require_creds("dashboards_query_search_field").is_none() {
        return;
    }
    // GET /api/v1/olly-kb/queries/by-field — returns results for fields referenced in dashboards
    let v = harness::run_ok_json(&[
        "dashboards",
        "query-search",
        "--field",
        "team_id",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    let arr = v.as_array().expect("should be a JSON array");
    assert!(
        !arr.is_empty(),
        "dashboards query-search --field 'team_id' should return at least one result"
    );
    harness::assert_array_of_objects_with_keys(&v, &["query_text", "similarity", "dashboard_name"]);
}

#[test]
#[ignore]
fn dashboards_search_exits_ok() {
    if harness::require_creds("dashboards_search").is_none() {
        return;
    }
    // GET /api/v1/olly-kb/dashboards/semantic-search — may return empty for accounts
    // without indexed dashboard embeddings; CLI must still exit 0.
    let output = harness::cx()
        .args([
            "dashboards",
            "search",
            "data usage",
            "--limit",
            "5",
            "-o",
            "json",
        ])
        .output()
        .expect("failed to execute cx");
    assert!(
        output.status.success(),
        "dashboards search should exit 0 even with empty results"
    );
}
