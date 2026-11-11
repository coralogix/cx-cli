use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn service_catalog_entity_types() {
    if harness::require_creds("service_catalog_entity_types").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["service-catalog", "entity-types", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["entity_type", "id", "display_name"]);
}

#[test]
#[ignore]
fn service_catalog_schema() {
    if harness::require_creds("service_catalog_schema").is_none() {
        return;
    }
    let Some(entity_type) = discover_entity_type() else {
        eprintln!("[e2e] skipping service_catalog_schema: no entity types on test team");
        return;
    };
    let v = harness::run_ok_json(&["service-catalog", "schema", &entity_type, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["columns", "groupable_labels", "filterable_labels"]);
    harness::assert_array_of_objects_with_keys(&v["columns"], &["id", "display_name"]);
}

#[test]
#[ignore]
fn service_catalog_entities() {
    if harness::require_creds("service_catalog_entities").is_none() {
        return;
    }
    let Some(entity_type) = discover_entity_type() else {
        eprintln!("[e2e] skipping service_catalog_entities: no entity types on test team");
        return;
    };
    let v = harness::run_ok_json(&["service-catalog", "entities", &entity_type, "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["name", "system", "last_seen"]);
}

#[test]
#[ignore]
fn service_catalog_data() {
    if harness::require_creds("service_catalog_data").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data: no entity types/columns on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        harness::SHORT_WINDOW_START,
        "--end",
        "now",
        "--column",
        &column,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["rows", "columns"]);
    harness::assert_array(&v["rows"]);
}

#[test]
#[ignore]
fn service_catalog_entity_data() {
    if harness::require_creds("service_catalog_entity_data").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!(
            "[e2e] skipping service_catalog_entity_data: no entity types/columns on test team"
        );
        return;
    };
    let Some(entity_id) = discover_entity_id(&entity_type) else {
        eprintln!(
            "[e2e] skipping service_catalog_entity_data: no entities of type '{entity_type}' on test team"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "entity-data",
        &entity_type,
        &entity_id,
        "--start",
        harness::SHORT_WINDOW_START,
        "--end",
        "now",
        "--column",
        &column,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["rows", "columns"]);
}

/// Discover an entity type from `service-catalog entity-types`. Cached so
/// multiple tests don't each pay for the call.
fn discover_entity_type() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("service_catalog_discover_entity_type")?;
            let stdout = harness::run_ok(&["service-catalog", "entity-types", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find_map(|item| item.get("entity_type")?.as_str().map(String::from))
        })
        .clone()
}

/// Discover an `(entity_type, column_id)` pair via `entity-types` + `schema`,
/// skipping entity types whose schema declares `required_filters` (e.g.
/// `transaction` requires `service_name`) since supplying those would need
/// another round of discovery this test isn't set up to do. Cached across
/// the `data` and `entity-data` tests.
fn discover_entity_type_and_column() -> Option<(String, String)> {
    static CACHE: OnceLock<Option<(String, String)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("service_catalog_discover_entity_type_and_column")?;
            let stdout = harness::run_ok(&["service-catalog", "entity-types", "-o", "json"]);
            let entity_types = harness::parse_json(&stdout)?;
            entity_types.as_array()?.iter().find_map(|item| {
                let entity_type = item.get("entity_type")?.as_str()?.to_string();
                let stdout =
                    harness::run_ok(&["service-catalog", "schema", &entity_type, "-o", "json"]);
                let schema = harness::parse_json(&stdout)?;
                let has_required_filters = schema
                    .get("required_filters")
                    .and_then(|v| v.as_array())
                    .is_some_and(|arr| !arr.is_empty());
                if has_required_filters {
                    return None;
                }
                let column = schema
                    .get("columns")?
                    .as_array()?
                    .first()?
                    .get("id")?
                    .as_str()?
                    .to_string();
                Some((entity_type, column))
            })
        })
        .clone()
}

/// Discover a known entity name for `entity_type` via `service-catalog
/// entities`. Not cached on its own - only called once, by
/// `service_catalog_entity_data`.
fn discover_entity_id(entity_type: &str) -> Option<String> {
    let stdout = harness::run_ok(&["service-catalog", "entities", entity_type, "-o", "json"]);
    let v = harness::parse_json(&stdout)?;
    v.as_array()?
        .iter()
        .find_map(|item| item.get("name")?.as_str().map(String::from))
}

/// `--aggregation timeseries` answers with one series per entity and column.
#[test]
#[ignore]
fn service_catalog_data_timeseries() {
    if harness::require_creds("service_catalog_data_timeseries").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data_timeseries: no entity types/columns");
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--aggregation",
        "timeseries",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["series"]);
    harness::assert_array_of_objects_with_keys(&v["series"], &["columnId", "labels", "datapoints"]);
    for series in v["series"].as_array().unwrap() {
        assert_eq!(series["columnId"], column, "{series}");
        // A bucket with no data carries only its timestamp.
        harness::assert_array_of_objects_with_keys(&series["datapoints"], &["timestampSeconds"]);
    }

    // The table-only controls are refused up front for timeseries.
    let output = harness::cx()
        .args([
            "service-catalog",
            "data",
            &entity_type,
            "--start",
            "now-30m",
            "--end",
            "now",
            "--column",
            &column,
            "--aggregation",
            "timeseries",
            "--limit",
            "2",
        ])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only apply to --aggregation table"));
}

/// `--limit` / `--sort-column` / `--sort-order` rank and cap table rows; two
/// columns come back on every row and in the `columns` metadata.
#[test]
#[ignore]
fn service_catalog_data_table_sorted_and_limited() {
    if harness::require_creds("service_catalog_data_table_sorted_and_limited").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data_table_sorted_and_limited: no entity types");
        return;
    };
    let Some(second) = discover_second_column(&entity_type, &column) else {
        eprintln!("[e2e] skipping service_catalog_data_table_sorted_and_limited: only one column");
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--column",
        &second,
        "--aggregation",
        "table",
        "--limit",
        "2",
        "--sort-column",
        &column,
        "--sort-order",
        "desc",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["rows", "columns"]);
    let rows = v["rows"].as_array().unwrap();
    assert!(rows.len() <= 2, "--limit 2: {v}");
    for row in rows {
        assert!(
            row.get(&column).is_some() && row.get(&second).is_some(),
            "{row}"
        );
    }
    let ids: Vec<&str> = v["columns"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert_eq!(ids, [column.as_str(), second.as_str()]);
    let numeric: Vec<f64> = rows.iter().filter_map(|r| r[&column].as_f64()).collect();
    assert!(
        numeric.windows(2).all(|w| w[0] >= w[1]),
        "desc order: {numeric:?}"
    );

    let text = harness::run_ok_nonempty(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--limit",
        "2",
        "--sort-column",
        &column,
        "-o",
        "text",
    ]);
    assert!(!String::from_utf8_lossy(&text).trim().is_empty());
}

/// `--group-by` adds the label to every row's identity; `--filter` on the
/// same label keeps only rows with that value. Both for `data` and for
/// `entity-data` (timeseries).
#[test]
#[ignore]
fn service_catalog_data_group_by_and_filter() {
    if harness::require_creds("service_catalog_data_group_by_and_filter").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data_group_by_and_filter: no entity types");
        return;
    };
    let Some(label) = discover_group_and_filter_label(&entity_type) else {
        eprintln!("[e2e] skipping service_catalog_data_group_by_and_filter: no label is both groupable and filterable");
        return;
    };
    let grouped = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--group-by",
        &label,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&grouped, &["rows", "columns"]);
    let rows = grouped["rows"].as_array().unwrap();
    // Entities without the label come back with an empty identity, so only
    // the rows that carry it can be checked.
    let Some(value) = rows
        .iter()
        .find_map(|r| r[&label].as_str().map(String::from))
    else {
        eprintln!("[e2e] skipping the filter half: no rows with a '{label}' value in the window");
        return;
    };

    let filter = format!("{label}={value}");
    let filtered = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--group-by",
        &label,
        "--filter",
        &filter,
        "-o",
        "json",
    ]);
    let rows = filtered["rows"].as_array().unwrap();
    assert!(!rows.is_empty(), "{filtered}");
    for row in rows {
        assert_eq!(
            row[&label], value,
            "--filter {filter} must keep only that value: {row}"
        );
    }

    let Some(entity_id) = discover_entity_id(&entity_type) else {
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "entity-data",
        &entity_type,
        &entity_id,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--group-by",
        &label,
        "--filter",
        &filter,
        "--aggregation",
        "timeseries",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["series"]);
    for series in v["series"].as_array().unwrap() {
        assert_eq!(series["labels"][&label], value, "{series}");
    }
    harness::run_ok(&[
        "service-catalog",
        "entity-data",
        &entity_type,
        &entity_id,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "-o",
        "text",
    ]);
    harness::run_ok_nonempty(&["service-catalog", "entity-types", "-o", "text"]);
    harness::run_ok_nonempty(&["service-catalog", "schema", &entity_type, "-o", "toon"]);
}

/// Client-side validation runs before any request.
#[test]
#[ignore]
fn service_catalog_rejects_bad_flags_locally() {
    if harness::require_creds("service_catalog_rejects_bad_flags_locally").is_none() {
        return;
    }
    for (args, expected) in [
        (
            vec![
                "data", "service", "--start", "now", "--end", "now-1h", "--column", "p99",
            ],
            "must be strictly after",
        ),
        (
            vec![
                "data",
                "service",
                "--start",
                "now-1h",
                "--end",
                "now",
                "--column",
                "p99",
                "--aggregation",
                "pie",
            ],
            "unknown --aggregation",
        ),
        (
            vec![
                "data",
                "service",
                "--start",
                "now-1h",
                "--end",
                "now",
                "--column",
                "p99",
                "--filter",
                "no-equals",
            ],
            "invalid --filter",
        ),
        (vec!["schema", "cx-no-such-entity-type"], "entity type"),
    ] {
        let output = harness::cx()
            .arg("service-catalog")
            .args(&args)
            .output()
            .expect("cx must run");
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.to_lowercase().contains(expected),
            "{args:?}: {stderr}"
        );
    }
}

/// A second column id of the same entity type (for multi-column requests).
fn discover_second_column(entity_type: &str, first: &str) -> Option<String> {
    let stdout = harness::run_ok(&["service-catalog", "schema", entity_type, "-o", "json"]);
    let schema = harness::parse_json(&stdout)?;
    schema
        .get("columns")?
        .as_array()?
        .iter()
        .filter_map(|c| c.get("id")?.as_str())
        .find(|id| *id != first)
        .map(String::from)
}

/// A label that can be used both with `--group-by` and `--filter`.
fn discover_group_and_filter_label(entity_type: &str) -> Option<String> {
    let stdout = harness::run_ok(&["service-catalog", "schema", entity_type, "-o", "json"]);
    let schema = harness::parse_json(&stdout)?;
    let filterable: Vec<String> = schema
        .get("filterable_labels")?
        .as_array()?
        .iter()
        .filter_map(|l| l.as_str().map(String::from))
        .collect();
    schema
        .get("groupable_labels")?
        .as_array()?
        .iter()
        .filter_map(|l| l.as_str())
        .find(|l| filterable.iter().any(|f| f == l))
        .map(String::from)
}

// `service-catalog` has no mutating subcommands, so there is nothing
// deliberately uncovered here - all five read-only subcommands
// (entity-types, schema, entities, data, entity-data) are exercised above.
