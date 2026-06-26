use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn metrics_query() {
    if harness::require_creds("metrics_query").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["metrics", "query", "up", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["metric", "value"]);
}

#[test]
#[ignore]
fn metrics_query_range() {
    if harness::require_creds("metrics_query_range").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "metrics",
        "query-range",
        "up",
        "--start",
        harness::SHORT_WINDOW_START,
        "--step",
        "1m",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&v, &["metric", "values"]);
}

#[test]
#[ignore]
fn metrics_search_name() {
    if harness::require_creds("metrics_search_name").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["metrics", "search", "--name", "*", "-o", "json"]);
    harness::assert_array_of_strings(&v);
}

#[test]
#[ignore]
fn metrics_get_labels() {
    if harness::require_creds("metrics_get_labels").is_none() {
        return;
    }
    let Some(metric) = discover_metric_name() else {
        eprintln!("[e2e] skipping metrics_get_labels: no metrics available on test team");
        return;
    };
    let v = harness::run_ok_json(&["metrics", "get-labels", &metric, "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["label"]);
}

/// Discover a metric name from `metrics search --name '*' -o json`.
/// The name-search rendering emits a top-level array of strings.
fn discover_metric_name() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["metrics", "search", "--name", "*", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?.first()?.as_str().map(String::from)
        })
        .clone()
}
