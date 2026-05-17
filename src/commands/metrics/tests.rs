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

// ── range_samples_to_toon_rows ────────────────────────────────────────────────

/// Build a tagged JSON row that range_samples_to_toon_rows expects.
fn make_range_row(metric: &[(&str, &str)], values: &[(u64, &str)], profile: Option<&str>) -> Value {
    let metric_obj: serde_json::Map<String, Value> = metric
        .iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect();
    let values_arr: Vec<Value> = values.iter().map(|(ts, val)| json!([ts, val])).collect();
    let mut row = json!({
        "metric": metric_obj,
        "values": values_arr,
    });
    if let Some(p) = profile {
        if let Value::Object(ref mut m) = row {
            m.insert("profile".to_string(), json!(p));
        }
    }
    row
}

#[test]
fn range_empty_input_returns_empty_vec() {
    let rows = range_samples_to_toon_rows(&[], false);
    assert!(rows.is_empty());
}

#[test]
fn range_single_series_single_point() {
    // 1 series, 1 point → 1 row; timestamp becomes a column header containing the value.
    let sample = make_range_row(&[("job", "api")], &[(1_719_014_400, "42")], None);
    let rows = range_samples_to_toon_rows(&[sample], false);

    assert_eq!(rows.len(), 1);
    let obj = rows[0].as_object().unwrap();
    assert_eq!(obj["job"], json!("api"));
    // The timestamp column key is the ISO string.
    assert_eq!(obj["2024-06-22T00:00:00Z"], json!("42"));
    // No separate "timestamp" or "value" columns.
    assert!(!obj.contains_key("timestamp"));
    assert!(!obj.contains_key("value"));
}

#[test]
fn range_single_series_multiple_points() {
    // 1 series, 3 points → 1 row with 3 timestamp columns.
    let sample = make_range_row(
        &[("job", "api")],
        &[
            (1_719_014_400, "1"),
            (1_719_014_460, "2"),
            (1_719_014_520, "3"),
        ],
        None,
    );
    let rows = range_samples_to_toon_rows(&[sample], false);

    assert_eq!(rows.len(), 1);
    let obj = rows[0].as_object().unwrap();
    assert_eq!(obj["job"], json!("api"));
    assert_eq!(obj["2024-06-22T00:00:00Z"], json!("1"));
    assert_eq!(obj["2024-06-22T00:01:00Z"], json!("2"));
    assert_eq!(obj["2024-06-22T00:02:00Z"], json!("3"));
}

#[test]
fn range_multiple_series_same_timestamps() {
    // 2 series sharing the same timestamps → 2 rows with shared timestamp columns.
    let samples = vec![
        make_range_row(
            &[("job", "a")],
            &[(1_719_014_400, "10"), (1_719_014_460, "20")],
            None,
        ),
        make_range_row(
            &[("job", "b")],
            &[(1_719_014_400, "30"), (1_719_014_460, "40")],
            None,
        ),
    ];
    let rows = range_samples_to_toon_rows(&samples, false);

    assert_eq!(rows.len(), 2);
    let r0 = rows[0].as_object().unwrap();
    let r1 = rows[1].as_object().unwrap();
    assert_eq!(r0["job"], json!("a"));
    assert_eq!(r0["2024-06-22T00:00:00Z"], json!("10"));
    assert_eq!(r0["2024-06-22T00:01:00Z"], json!("20"));
    assert_eq!(r1["job"], json!("b"));
    assert_eq!(r1["2024-06-22T00:00:00Z"], json!("30"));
    assert_eq!(r1["2024-06-22T00:01:00Z"], json!("40"));
}

#[test]
fn range_multiple_series_differing_labels_padded() {
    let samples = vec![
        make_range_row(&[("job", "a")], &[(1_719_014_400, "1")], None),
        make_range_row(
            &[("job", "b"), ("pod", "p1")],
            &[(1_719_014_400, "2")],
            None,
        ),
    ];
    let rows = range_samples_to_toon_rows(&samples, false);

    assert_eq!(rows.len(), 2);
    // Missing label padded with empty string.
    assert_eq!(rows[0].as_object().unwrap()["pod"], json!(""));
    assert_eq!(rows[1].as_object().unwrap()["pod"], json!("p1"));
}

#[test]
fn range_sparse_timestamps_padded() {
    // Series 0 has T1+T2; Series 1 has only T1 → T2 padded with "" for series 1.
    let samples = vec![
        make_range_row(
            &[("job", "a")],
            &[(1_719_014_400, "1"), (1_719_014_460, "2")],
            None,
        ),
        make_range_row(&[("job", "b")], &[(1_719_014_400, "3")], None),
    ];
    let rows = range_samples_to_toon_rows(&samples, false);

    assert_eq!(rows.len(), 2);
    // Series b is missing the second timestamp.
    assert_eq!(
        rows[1].as_object().unwrap()["2024-06-22T00:01:00Z"],
        json!("")
    );
}

#[test]
fn range_label_keys_sorted_alphabetically() {
    let sample = make_range_row(
        &[("zzz", "z"), ("aaa", "a"), ("mmm", "m")],
        &[(1_719_014_400, "0")],
        None,
    );
    let rows = range_samples_to_toon_rows(&[sample], false);
    let keys: Vec<&str> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    // Sorted label keys first, then the timestamp column.
    assert_eq!(keys, vec!["aaa", "mmm", "zzz", "2024-06-22T00:00:00Z"]);
}

#[test]
fn range_timestamp_is_iso8601() {
    // 1719014400 == 2024-06-22T00:00:00Z — verifies epoch→ISO conversion.
    let sample = make_range_row(&[("job", "x")], &[(1_719_014_400, "1")], None);
    let rows = range_samples_to_toon_rows(&[sample], false);
    let keys: Vec<&str> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert!(keys.contains(&"2024-06-22T00:00:00Z"));
}

#[test]
fn range_with_profile_column() {
    let sample = make_range_row(&[("job", "api")], &[(1_719_014_400, "1")], Some("prod"));
    let rows = range_samples_to_toon_rows(&[sample], true);
    let obj = rows[0].as_object().unwrap();
    assert_eq!(obj["profile"], json!("prod"));
    // profile appears between label keys and timestamp columns.
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["job", "profile", "2024-06-22T00:00:00Z"]);
}

#[test]
fn range_without_profile_column() {
    let sample = make_range_row(&[("job", "api")], &[(1_719_014_400, "1")], None);
    let rows = range_samples_to_toon_rows(&[sample], false);
    let obj = rows[0].as_object().unwrap();
    assert!(!obj.contains_key("profile"));
}

#[test]
fn range_column_order() {
    // Full column order: sorted labels → profile → timestamps.
    let sample = make_range_row(
        &[("job", "api"), ("instance", "h1")],
        &[(1_719_014_400, "1"), (1_719_014_460, "2")],
        Some("prod"),
    );
    let rows = range_samples_to_toon_rows(&[sample], true);
    let keys: Vec<&str> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "instance",
            "job",
            "profile",
            "2024-06-22T00:00:00Z",
            "2024-06-22T00:01:00Z"
        ]
    );
}
