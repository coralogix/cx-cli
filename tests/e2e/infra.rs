use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn infra_types() {
    if harness::require_creds("infra_types").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["infra", "resources", "types", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["category", "type", "label"]);
    // `resourceType` is returned by the API but deliberately not surfaced.
    for item in v.as_array().unwrap_or(&vec![]) {
        assert!(
            item.get("resource_type").is_none(),
            "resource_type must not be surfaced, got: {item}"
        );
    }
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
    // `list` returns an envelope, not a bare array: callers page with
    // `--start-row`/`--end-row` and `total_count` is their only stop condition.
    harness::assert_object_with_keys(&v, &["total_count", "returned_count", "resources"]);
    assert!(
        v["total_count"].is_i64(),
        "total_count should be a number, got {:?} - callers have no stop condition without it",
        v["total_count"]
    );
    harness::assert_array_of_objects_with_keys(&v["resources"], &["resource_id", "name"]);
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
    harness::assert_array_of_objects_with_keys(&v, &["resource_id", "timestamp", "status"]);
}

/// The endpoint takes a list, and repeats are read once, so the same id twice
/// must answer as if it were given once rather than erroring or doubling up.
#[test]
#[ignore]
fn infra_health_history_takes_several_ids() {
    if harness::require_creds("infra_health_history_takes_several_ids").is_none() {
        return;
    }
    let Some(id) = discover_resource_id() else {
        eprintln!(
            "[e2e] skipping infra_health_history_takes_several_ids: no resources on test team"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "infra",
        "resources",
        "health-history",
        &id,
        &id,
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, &["resource_id", "timestamp", "status"]);
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
    // The document shape is source-specific, so only verify exit 0 + the
    // envelope's own keys.
    let stdout = harness::run_ok(&["infra", "resources", "raw-data", &id, "-o", "json"]);
    let v = harness::parse_json(&stdout).expect("raw-data should emit valid JSON");
    assert!(v.get("raw_data").is_some(), "no raw_data key: {v}");
    assert!(
        v.get("version_timestamp").is_some(),
        "no version_timestamp key: {v}"
    );
}

/// The as-of query returns the newest version at or before the instant, so a
/// far-past timestamp is a valid ask that may legitimately find nothing.
#[test]
#[ignore]
fn infra_raw_data_at_a_timestamp() {
    if harness::require_creds("infra_raw_data_at_a_timestamp").is_none() {
        return;
    }
    let Some(id) = discover_resource_id() else {
        eprintln!("[e2e] skipping infra_raw_data_at_a_timestamp: no resources on test team");
        return;
    };
    let stdout = harness::run_ok(&[
        "infra",
        "resources",
        "raw-data",
        &id,
        "--timestamp",
        "now-1d",
        "-o",
        "json",
    ]);
    let v = harness::parse_json(&stdout).expect("raw-data --timestamp should emit valid JSON");
    assert!(v.get("raw_data").is_some(), "no raw_data key: {v}");
}

/// A sweep over a real resource. An empty result is a valid answer: it means
/// nothing changed in the window, so only exit 0 and the row shape are checked.
#[test]
#[ignore]
fn infra_config_changes() {
    if harness::require_creds("infra_config_changes").is_none() {
        return;
    }
    let Some(id) = discover_resource_id() else {
        eprintln!("[e2e] skipping infra_config_changes: no resources on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "infra",
        "resources",
        "config-changes",
        "--resource-id",
        &id,
        "--from",
        "now-7d",
        "-o",
        "json",
    ]);
    let rows = v.as_array().expect("config-changes should emit an array");
    for row in rows {
        assert!(row.get("resource_id").is_some(), "no resource_id: {row}");
        assert!(row.get("outcome").is_some(), "no outcome: {row}");
    }
}

/// The diff answers for every resource asked about, including unchanged ones,
/// so this one does expect rows.
#[test]
#[ignore]
fn infra_config_diff() {
    if harness::require_creds("infra_config_diff").is_none() {
        return;
    }
    let Some(id) = discover_resource_id() else {
        eprintln!("[e2e] skipping infra_config_diff: no resources on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "infra",
        "resources",
        "config-diff",
        "--resource-id",
        &id,
        "--from",
        "now-7d",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, &["resource_id", "outcome", "changes"]);
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
            v.get("resources")?
                .as_array()?
                .iter()
                .find_map(|item| item.get("resource_id")?.as_str().map(String::from))
        })
        .clone()
}

#[test]
#[ignore]
fn infra_filters() {
    if harness::require_creds("infra_filters").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["infra", "resources", "filters", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["name", "kind", "wildcard", "types"]);
}

#[test]
#[ignore]
fn infra_filters_scoped_to_one_type() {
    if harness::require_creds("infra_filters_scoped").is_none() {
        return;
    }
    let Some((category, resource_type)) = discover_category_type() else {
        eprintln!(
            "[e2e] skipping infra_filters_scoped_to_one_type: no resource types on test team"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "infra",
        "resources",
        "filters",
        "--category",
        &category,
        "--type",
        &resource_type,
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, &["name", "kind", "wildcard"]);
}

#[test]
#[ignore]
fn infra_list_with_one_filter() {
    if harness::require_creds("infra_list_with_one_filter").is_none() {
        return;
    }
    let Some((attribute, value)) = discover_closed_set_filter() else {
        eprintln!(
            "[e2e] skipping infra_list_with_one_filter: no closed-set attribute on test team"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "infra",
        "resources",
        "list",
        "--match-all",
        &format!("{attribute}={value}"),
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["total_count", "returned_count", "resources"]);
}

#[test]
#[ignore]
fn infra_list_with_a_nested_filter() {
    if harness::require_creds("infra_list_with_a_nested_filter").is_none() {
        return;
    }
    let Some([(first, first_value), (second, second_value), (third, third_value)]) =
        discover_closed_set_filters()
    else {
        eprintln!(
            "[e2e] skipping infra_list_with_a_nested_filter: need three closed-set attributes"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "infra",
        "resources",
        "list",
        "--match-all",
        &format!("{first}={first_value}"),
        "--match-any",
        &format!("{second}={second_value}"),
        "--match-any",
        &format!("{third}={third_value}"),
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["total_count", "returned_count", "resources"]);
}

#[test]
#[ignore]
fn infra_list_rows_carry_their_classification() {
    if harness::require_creds("infra_list_rows_carry_classification").is_none() {
        return;
    }
    let Some((category, resource_type)) = discover_category_type() else {
        eprintln!("[e2e] skipping infra_list_rows_carry_their_classification: no resource types");
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
    let rows = v["resources"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        eprintln!("[e2e] skipping the row assertions: {category}/{resource_type} is empty");
        return;
    }
    for row in &rows {
        assert_eq!(
            row.get("category").and_then(|c| c.as_str()),
            Some(category.as_str()),
            "a pinned request still returns the category on every row, got: {row}"
        );
        assert_eq!(
            row.get("type").and_then(|t| t.as_str()),
            Some(resource_type.as_str()),
            "a pinned request still returns the type on every row, got: {row}"
        );
    }
}

/// Three distinct attributes, so a nested filter can name one per flag - an
/// attribute repeated across the two groups is refused.
fn discover_closed_set_filters() -> Option<[(String, String); 3]> {
    static CACHE: OnceLock<Option<[(String, String); 3]>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("infra_discover_filters_for_nesting")?;
            let stdout = harness::run_ok(&["infra", "resources", "filters", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            let mut found: Vec<(String, String)> = Vec::new();
            for item in v.as_array()? {
                let Some(name) = item.get("name").and_then(|n| n.as_str()) else {
                    continue;
                };
                let Some(value) = item
                    .get("values")
                    .and_then(|v| v.as_array())
                    .and_then(|v| v.first())
                    .and_then(|v| v.as_str())
                else {
                    continue;
                };
                if found.iter().any(|(seen, _)| seen == name) {
                    continue;
                }
                found.push((name.to_string(), value.to_string()));
                if found.len() == 3 {
                    return Some([found[0].clone(), found[1].clone(), found[2].clone()]);
                }
            }
            None
        })
        .clone()
}

fn discover_closed_set_filter() -> Option<(String, String)> {
    static CACHE: OnceLock<Option<(String, String)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("infra_discover_filters")?;
            let stdout = harness::run_ok(&["infra", "resources", "filters", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?.iter().find_map(|item| {
                let name = item.get("name")?.as_str()?.to_string();
                let value = item
                    .get("values")?
                    .as_array()?
                    .first()?
                    .as_str()?
                    .to_string();
                Some((name, value))
            })
        })
        .clone()
}

// `infra` has no mutating subcommands, so there is nothing deliberately
// uncovered here - all four read-only subcommands are exercised above.
