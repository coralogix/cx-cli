use crate::harness;

#[test]
#[ignore]
fn metrics_query() {
    if harness::require_creds("metrics_query").is_none() {
        return;
    }
    harness::run_ok_json(&["metrics", "query", "up", "-o", "json"]);
}

#[test]
#[ignore]
fn metrics_query_range() {
    if harness::require_creds("metrics_query_range").is_none() {
        return;
    }
    harness::run_ok_json(&[
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
}

#[test]
#[ignore]
fn metrics_search_name() {
    if harness::require_creds("metrics_search_name").is_none() {
        return;
    }
    harness::run_ok_json(&["metrics", "search", "--name", "*", "-o", "json"]);
}

#[test]
#[ignore]
fn metrics_get_labels() {
    if harness::require_creds("metrics_get_labels").is_none() {
        return;
    }
    let Some(metric) = harness::discover_metric_name() else {
        eprintln!("[e2e] skipping metrics_get_labels: no metrics available in staging");
        return;
    };
    harness::run_ok_json(&["metrics", "get-labels", &metric, "-o", "json"]);
}
