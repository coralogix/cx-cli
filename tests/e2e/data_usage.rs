use crate::harness;

#[test]
#[ignore]
fn data_usage_summary() {
    if harness::require_creds("data_usage_summary").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["usage", "summary", "-o", "json"]);
}

#[test]
#[ignore]
fn data_usage_daily() {
    if harness::require_creds("data_usage_daily").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&[
        "usage",
        "daily",
        "--type",
        "processed-gbs",
        "--start",
        "now-7d",
        "--end",
        "now",
        "-o",
        "json",
    ]);
}

#[test]
#[ignore]
fn data_usage_logs_count() {
    if harness::require_creds("data_usage_logs_count").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "usage",
        "logs-count",
        "--start",
        "now-1h",
        "--end",
        "now",
        "-o",
        "json",
    ]);
    assert_count_result(&v, "logsCount");
}

#[test]
#[ignore]
fn data_usage_spans_count() {
    if harness::require_creds("data_usage_spans_count").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "usage",
        "spans-count",
        "--start",
        "now-1h",
        "--end",
        "now",
        "-o",
        "json",
    ]);
    assert_count_result(&v, "spansCount");
}

fn assert_count_result(v: &serde_json::Value, key: &str) {
    let rows = v
        .get("result")
        .and_then(|result| result.get(key))
        .and_then(|value| value.as_array())
        .unwrap_or_else(|| panic!("expected result.{key} array, got: {v}"));

    for (i, row) in rows.iter().enumerate() {
        assert!(
            row.get("timestamp").is_some(),
            "row {i} missing timestamp: {row}"
        );
    }
}
