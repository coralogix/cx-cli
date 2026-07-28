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
fn data_usage_capabilities() {
    if harness::require_creds("data_usage_capabilities").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["usage", "capabilities", "-o", "json"]);
    assert!(
        v.get("supportedLabels").is_some(),
        "expected supportedLabels in capabilities response: {v}"
    );
    assert!(
        v.get("supportedMeasurements").is_some(),
        "expected supportedMeasurements in capabilities response: {v}"
    );
}

#[test]
#[ignore]
fn data_usage_query() {
    if harness::require_creds("data_usage_query").is_none() {
        return;
    }

    let file_path = std::env::temp_dir().join(format!(
        "cx-data-usage-query-e2e-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &file_path,
        r#"{"daily":{"relativeRange":"DAILY_RELATIVE_RANGE_LAST_7_DAYS"},"limit":{"perBucket":1}}"#,
    )
    .unwrap();

    let file_path = file_path.to_str().expect("temporary path is valid UTF-8");
    let v = harness::run_ok_json(&["usage", "query", "--from-file", file_path, "-o", "json"]);
    std::fs::remove_file(file_path).unwrap();

    assert!(
        v.get("queryRange").is_some(),
        "expected queryRange in query response: {v}"
    );
    assert!(
        v.get("buckets").is_some(),
        "expected buckets in query response: {v}"
    );
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
