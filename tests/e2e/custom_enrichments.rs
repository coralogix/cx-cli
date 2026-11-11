use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn custom_enrichments_list() {
    if harness::require_creds("custom_enrichments_list").is_none() {
        return;
    }
    // The writable e2e team starts with no custom enrichment tables; the
    // lifecycle test below creates its own, so only the shape is asserted.
    let v = harness::run_ok_json(&["enrichments", "custom", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn custom_enrichments_get() {
    if harness::require_creds("custom_enrichments_get").is_none() {
        return;
    }
    let id = match discover_custom_enrichment_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping custom_enrichments_get: no custom enrichments on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["enrichments", "custom", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

fn discover_custom_enrichment_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("custom_enrichments_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["enrichments", "custom", "list", "-o", "json"]);
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

/// A small CSV lookup table. `size` is the UTF-8 byte length of `textual`.
pub fn table_body(name: &str, description: &str, rows: &[(&str, &str)]) -> Value {
    let mut textual = String::from("key,value");
    for (k, v) in rows {
        textual.push('\n');
        textual.push_str(k);
        textual.push(',');
        textual.push_str(v);
    }
    json!({
        "name": name,
        "description": description,
        "file": {
            "textual": textual,
            "extension": "csv",
            "name": "lookup.csv",
            "size": textual.len()
        }
    })
}

/// Create a table and return its numeric id as text. Shared with the
/// enrichment-rule tests in `enrichments.rs`.
pub fn create_table(name: &str) -> String {
    let body = table_body(
        name,
        "e2e lookup table",
        &[("cx-e2e-k1", "cx-e2e-v1"), ("cx-e2e-k2", "cx-e2e-v2")],
    );
    let file = harness::JsonFile::new("ce", &body);
    let created = harness::run_write_json(&[
        "enrichments",
        "custom",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_eq!(created["name"], name, "create should echo our name");
    harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("custom enrichment create returned no id: {created}"))
}

/// `get` wraps the table as `{"customEnrichment": {...}}`.
fn table_of(get_response: &Value) -> &Value {
    get_response.get("customEnrichment").unwrap_or(get_response)
}

#[test]
#[ignore]
fn custom_enrichments_lifecycle() {
    if harness::require_write_creds("custom_enrichments_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("ce");

    // create
    let id = create_table(&name);
    let mut cleanup = harness::Cleanup::new(&["enrichments", "custom", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["enrichments", "custom", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name"]);
    assert_eq!(table_of(&got)["name"], name);

    // list contains it
    let list = harness::run_ok_json(&["enrichments", "custom", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "name", &name).is_some(),
        "custom enrichment list should contain {name}"
    );

    // search: one matching row, then none
    let hit = harness::run_ok_json(&[
        "enrichments",
        "custom",
        "search",
        "--id",
        &id,
        "--query",
        "cx-e2e-k2",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&hit, &["customEnrichmentId", "header", "matches"]);
    assert_eq!(hit["header"], json!("key,value"));
    assert_eq!(
        hit["matchCount"],
        json!(1),
        "search should find one row: {hit}"
    );
    assert_eq!(hit["matches"], json!(["cx-e2e-k2,cx-e2e-v2"]));
    let miss = harness::run_ok_json(&[
        "enrichments",
        "custom",
        "search",
        "--id",
        &id,
        "--query",
        "cx-e2e-no-such-row",
        "-o",
        "json",
    ]);
    assert_eq!(
        miss["matchCount"],
        json!(0),
        "search should find nothing: {miss}"
    );

    // update: new description and a third row
    let id_num: u64 = id.parse().expect("custom enrichment ids are numeric");
    let mut updated = table_body(
        &name,
        "e2e lookup table updated",
        &[
            ("cx-e2e-k1", "cx-e2e-v1"),
            ("cx-e2e-k2", "cx-e2e-v2"),
            ("cx-e2e-k3", "cx-e2e-v3"),
        ],
    );
    updated["customEnrichmentId"] = json!(id_num);
    let upd = harness::JsonFile::new("ce-upd", &updated);
    harness::run_write_ok(&[
        "enrichments",
        "custom",
        "update",
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["enrichments", "custom", "get", &id, "-o", "json"]);
    assert_eq!(
        table_of(&got)["description"],
        json!("e2e lookup table updated"),
        "update should change the description: {got}"
    );
    let hit = harness::run_ok_json(&[
        "enrichments",
        "custom",
        "search",
        "--id",
        &id,
        "--query",
        "cx-e2e-k3",
        "-o",
        "json",
    ]);
    assert_eq!(
        hit["matchCount"],
        json!(1),
        "updated table should hold the new row"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["enrichments", "custom", "get", &id]);
    harness::run_ok_nonempty(&["enrichments", "custom", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["enrichments", "custom", "list"]);
    harness::run_ok_nonempty(&[
        "enrichments",
        "custom",
        "search",
        "--id",
        &id,
        "--query",
        "cx-e2e-k1",
    ]);

    // delete, then get must fail
    harness::run_write_ok(&["enrichments", "custom", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["enrichments", "custom", "get", &id, "-o", "json"]);
    harness::run_fail(&[
        "enrichments",
        "custom",
        "search",
        "--id",
        &id,
        "--query",
        "cx-e2e-k1",
        "-o",
        "json",
    ]);
}

#[test]
#[ignore]
fn custom_enrichments_get_and_delete_missing_fail() {
    if harness::require_write_creds("custom_enrichments_get_and_delete_missing_fail").is_none() {
        return;
    }
    harness::run_fail(&["enrichments", "custom", "get", "1", "-o", "json"]);
    harness::run_fail(&["enrichments", "custom", "delete", "1", "--yes"]);
}
