use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn infra_types() {
    if harness::require_creds("infra_types").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["infra", "resources", "types", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["category", "type", "resource_type"]);
}

#[test]
#[ignore]
fn infra_list() {
    if harness::require_creds("infra_list").is_none() {
        return;
    }
    let Some((category, resource_type)) = discover_category_type() else {
        eprintln!("[e2e] skipping infra_list: no resource types on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "infra",
        "resources",
        "list",
        "--category",
        &category,
        "--type",
        &resource_type,
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, &["resource_id", "name"]);
}

#[test]
#[ignore]
fn infra_health_history() {
    if harness::require_creds("infra_health_history").is_none() {
        return;
    }
    let Some(id) = discover_resource_id() else {
        eprintln!("[e2e] skipping infra_health_history: no resources on test team");
        return;
    };
    let v = harness::run_ok_json(&["infra", "resources", "health-history", &id, "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["timestamp", "status"]);
}

#[test]
#[ignore]
fn infra_raw_data() {
    if harness::require_creds("infra_raw_data").is_none() {
        return;
    }
    let Some(id) = discover_resource_id() else {
        eprintln!("[e2e] skipping infra_raw_data: no resources on test team");
        return;
    };
    // The document shape is source-specific, so only verify exit 0 + valid JSON.
    let stdout = harness::run_ok(&["infra", "resources", "raw-data", &id, "-o", "json"]);
    harness::parse_json(&stdout).expect("raw-data should emit valid JSON");
}

/// Discover a (category, type) pair from `infra resources types`. Cached so
/// multiple tests don't each pay for the call.
fn discover_category_type() -> Option<(String, String)> {
    static CACHE: OnceLock<Option<(String, String)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("infra_discover_types")?;
            let stdout = harness::run_ok(&["infra", "resources", "types", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?.iter().find_map(|item| {
                let category = item.get("category")?.as_str()?.to_string();
                let resource_type = item.get("type")?.as_str()?.to_string();
                Some((category, resource_type))
            })
        })
        .clone()
}

/// Discover a resource id via `infra resources list` for the first available
/// resource type. Cached across the health-history and raw-data tests.
fn discover_resource_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let (category, resource_type) = discover_category_type()?;
            let stdout = harness::run_ok(&[
                "infra",
                "resources",
                "list",
                "--category",
                &category,
                "--type",
                &resource_type,
                "-o",
                "json",
            ]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find_map(|item| item.get("resource_id")?.as_str().map(String::from))
        })
        .clone()
}

// `infra` has no mutating subcommands, so there is nothing deliberately
// uncovered here - all four read-only subcommands are exercised above.
