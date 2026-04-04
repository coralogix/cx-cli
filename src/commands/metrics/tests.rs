use serde_json::json;

use super::*;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a tagged JSON row that instant_samples_to_toon_rows expects.
fn make_instant_row(metric: &[(&str, &str)], value: &str, profile: Option<&str>) -> Value {
    let metric_obj: serde_json::Map<String, Value> = metric
        .iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect();
    let mut row = json!({
        "metric": metric_obj,
        "value": [json!(1_700_000_000u64), json!(value)],
    });
    if let Some(p) = profile {
        if let Value::Object(ref mut m) = row {
            m.insert("profile".to_string(), json!(p));
        }
    }
    row
}

// ── instant_samples_to_toon_rows (multi-profile) ──────────────────────────────

#[test]
fn empty_input_returns_empty_vec() {
    let rows = instant_samples_to_toon_rows(&[], true);
    assert!(rows.is_empty());
}

#[test]
fn single_sample_single_label_with_profile() {
    let sample = make_instant_row(&[("job", "prometheus")], "1", Some("default"));
    let rows = instant_samples_to_toon_rows(&[sample], true);

    assert_eq!(rows.len(), 1);
    let obj = rows[0].as_object().unwrap();
    assert_eq!(obj["job"], json!("prometheus"));
    assert_eq!(obj["value"], json!("1"));
    assert_eq!(obj["profile"], json!("default"));
    assert!(!obj.contains_key("timestamp"));
}

#[test]
fn single_sample_multiple_labels_with_profile() {
    let sample = make_instant_row(
        &[("job", "node"), ("instance", "host:9100")],
        "42",
        Some("p1"),
    );
    let rows = instant_samples_to_toon_rows(&[sample], true);

    let obj = rows[0].as_object().unwrap();
    // Metric label keys come first sorted alphabetically, then "profile", then "value".
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["instance", "job", "profile", "value"]);
    assert_eq!(obj["instance"], json!("host:9100"));
    assert_eq!(obj["job"], json!("node"));
    assert_eq!(obj["value"], json!("42"));
}

#[test]
fn multiple_samples_same_labels_produce_uniform_rows_with_profile() {
    let samples = vec![
        make_instant_row(&[("job", "a"), ("instance", "h1")], "1", Some("default")),
        make_instant_row(&[("job", "b"), ("instance", "h2")], "2", Some("default")),
    ];
    let rows = instant_samples_to_toon_rows(&samples, true);

    assert_eq!(rows.len(), 2);
    for row in &rows {
        let keys: Vec<&str> = row
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        // instance, job, profile, value
        assert_eq!(keys.len(), 4);
    }
    assert_eq!(rows[0].as_object().unwrap()["value"], json!("1"));
    assert_eq!(rows[1].as_object().unwrap()["value"], json!("2"));
}

#[test]
fn multiple_samples_differing_labels_padded_with_empty_string_with_profile() {
    // One sample has an extra label "pod" that the other lacks.
    let samples = vec![
        make_instant_row(&[("job", "a")], "1", Some("default")),
        make_instant_row(&[("job", "b"), ("pod", "p1")], "2", Some("default")),
    ];
    let rows = instant_samples_to_toon_rows(&samples, true);

    assert_eq!(rows.len(), 2);

    let r0 = rows[0].as_object().unwrap();
    let r1 = rows[1].as_object().unwrap();

    // Both rows must carry the full key set: job, pod (metric labels sorted), then profile, value.
    assert_eq!(
        r0.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["job", "pod", "profile", "value"]
    );
    assert_eq!(
        r1.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["job", "pod", "profile", "value"]
    );

    // Missing label is filled with an empty string so toon can render a table.
    assert_eq!(r0["pod"], json!(""));
    assert_eq!(r1["pod"], json!("p1"));
}

#[test]
fn label_keys_are_sorted_alphabetically_before_profile_and_value() {
    let sample = make_instant_row(
        &[("zzz", "z"), ("aaa", "a"), ("mmm", "m")],
        "0",
        Some("default"),
    );
    let rows = instant_samples_to_toon_rows(&[sample], true);

    let keys: Vec<&str> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    // Metric label keys sorted first, then "profile", then "value" last.
    assert_eq!(keys, vec!["aaa", "mmm", "zzz", "profile", "value"]);
}

#[test]
fn value_column_is_always_last_with_profile() {
    let sample = make_instant_row(&[("a", "1"), ("z", "2")], "99", Some("default"));
    let rows = instant_samples_to_toon_rows(&[sample], true);
    let keys: Vec<&str> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.last(), Some(&"value"));
}

#[test]
fn multi_profile_samples_carry_distinct_profile_values() {
    let samples = vec![
        make_instant_row(&[("job", "api")], "1", Some("prod")),
        make_instant_row(&[("job", "api")], "2", Some("staging")),
    ];
    let rows = instant_samples_to_toon_rows(&samples, true);

    assert_eq!(rows[0].as_object().unwrap()["profile"], json!("prod"));
    assert_eq!(rows[1].as_object().unwrap()["profile"], json!("staging"));
}

// ── instant_samples_to_toon_rows (single-profile, no profile column) ──────────

#[test]
fn single_profile_sample_omits_profile_column() {
    let sample = make_instant_row(&[("job", "prometheus")], "1", None);
    let rows = instant_samples_to_toon_rows(&[sample], false);

    assert_eq!(rows.len(), 1);
    let obj = rows[0].as_object().unwrap();
    assert_eq!(obj["job"], json!("prometheus"));
    assert_eq!(obj["value"], json!("1"));
    assert!(!obj.contains_key("profile"));
}

#[test]
fn single_profile_multiple_labels_omits_profile_column() {
    let sample = make_instant_row(&[("job", "node"), ("instance", "host:9100")], "42", None);
    let rows = instant_samples_to_toon_rows(&[sample], false);

    let obj = rows[0].as_object().unwrap();
    // Metric label keys sorted alphabetically, then "value". No "profile".
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["instance", "job", "value"]);
    assert!(!obj.contains_key("profile"));
}

#[test]
fn single_profile_value_column_is_always_last() {
    let sample = make_instant_row(&[("a", "1"), ("z", "2")], "99", None);
    let rows = instant_samples_to_toon_rows(&[sample], false);
    let keys: Vec<&str> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.last(), Some(&"value"));
    assert!(!keys.contains(&"profile"));
}
