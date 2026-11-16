//! `cx search-fields` (Olly knowledge base: semantic field search and
//! search-by-value). The index is built per team and may be empty on the
//! test team, so these assert exit status and row shape, and log when the
//! index returned nothing.

use crate::harness;

const SEMANTIC_KEYS: &[&str] = &["dataprime_path", "description", "similarity"];
const VALUE_KEYS: &[&str] = &["key_matched", "value", "similarity_score"];

fn note_if_empty(label: &str, v: &serde_json::Value) {
    if v.as_array().is_some_and(|a| a.is_empty()) {
        eprintln!("[e2e] {label} returned no rows: field index not populated for this team");
    }
}

#[test]
#[ignore]
fn search_fields_name_logs() {
    if harness::require_creds("search_fields_name_logs").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "http response",
        "--dataset",
        "logs",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, SEMANTIC_KEYS);
    note_if_empty("search-fields 'http response'", &v);
}

#[test]
#[ignore]
fn search_fields_name_spans() {
    if harness::require_creds("search_fields_name_spans").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "trace id",
        "--dataset",
        "spans",
        "--limit",
        "3",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, SEMANTIC_KEYS);
    assert!(v.as_array().unwrap().len() <= 3);
    note_if_empty("search-fields 'trace id' --dataset spans", &v);
}

/// The explicit `--search-type semantic` spelling (default) and the
/// `status` query from the data brief.
#[test]
#[ignore]
fn search_fields_status_semantic_explicit() {
    if harness::require_creds("search_fields_status_semantic_explicit").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "status",
        "--search-type",
        "semantic",
        "--dataset",
        "logs",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, SEMANTIC_KEYS);
    assert!(v.as_array().unwrap().len() <= 5);
    note_if_empty("search-fields 'status'", &v);
    // Text and toon renderings of the same search must not fail.
    harness::run_ok(&["search-fields", "status", "-o", "text"]);
    harness::run_ok(&["search-fields", "status", "-o", "toon"]);
}

#[test]
#[ignore]
fn search_fields_value_logs() {
    if harness::require_creds("search_fields_value_logs").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "log",
        "-s",
        "value",
        "--dataset",
        "logs",
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, VALUE_KEYS);
    note_if_empty("search-fields 'log' -s value", &v);
}

/// `--dataset all` plus `--offset` are only meaningful for value search.
#[test]
#[ignore]
fn search_fields_value_all_datasets_with_offset() {
    if harness::require_creds("search_fields_value_all_datasets_with_offset").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "search-fields",
        "status",
        "--search-type",
        "value",
        "--dataset",
        "all",
        "--limit",
        "5",
        "--offset",
        "2",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, VALUE_KEYS);
    assert!(v.as_array().unwrap().len() <= 5);
    note_if_empty(
        "search-fields 'status' -s value --dataset all --offset 2",
        &v,
    );
    harness::run_ok(&[
        "search-fields",
        "status",
        "-s",
        "value",
        "--dataset",
        "spans",
        "-o",
        "text",
    ]);
}

/// `--dataset all` is rejected for the semantic search type before any
/// request is sent.
#[test]
#[ignore]
fn search_fields_semantic_rejects_dataset_all() {
    if harness::require_creds("search_fields_semantic_rejects_dataset_all").is_none() {
        return;
    }
    let output = harness::cx()
        .args(["search-fields", "status", "--dataset", "all", "-o", "json"])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--dataset all is only valid with -s value"),
        "{stderr}"
    );
}
