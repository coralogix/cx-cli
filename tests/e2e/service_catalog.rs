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

// `service-catalog` has no mutating subcommands, so there is nothing
// deliberately uncovered here - all five read-only subcommands
// (entity-types, schema, entities, data, entity-data) are exercised above.
