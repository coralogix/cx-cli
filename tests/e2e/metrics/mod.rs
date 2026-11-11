//! `cx metrics` against the test team's PromQL endpoint. The team ingests
//! otel-demo and `anthropic_*` metrics continuously, so a count over every
//! series is always positive and the `anthropic_*` name prefix always matches.

use std::sync::OnceLock;

use crate::harness;

/// Counts every series the team has right now.
const COUNT_ALL: &str = "count({__name__=~\".+\"})";

fn instant_value(row: &serde_json::Value) -> f64 {
    row["value"][1]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or_else(|| panic!("instant sample value should be a numeric string: {row}"))
}

#[test]
#[ignore]
fn metrics_query() {
    if harness::require_creds("metrics_query").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["metrics", "query", "up", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["metric", "value"]);
}

/// An instant query over every series has one sample with a positive value.
#[test]
#[ignore]
fn metrics_query_count_all_series_is_positive() {
    if harness::require_creds("metrics_query_count_all_series_is_positive").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["metrics", "query", COUNT_ALL, "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["metric", "value"]);
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 1, "count() yields a single sample: {v}");
    assert!(instant_value(&rows[0]) > 0.0, "{v}");
    assert!(
        rows[0]["value"][0].is_number(),
        "sample carries an epoch: {v}"
    );
}

/// --time evaluates at that instant (relative syntax is resolved by cx).
#[test]
#[ignore]
fn metrics_query_with_time() {
    if harness::require_creds("metrics_query_with_time").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "metrics", "query", COUNT_ALL, "--time", "now-5m", "-o", "json",
    ]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["metric", "value"]);
    let epoch = v[0]["value"][0].as_f64().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let age = now - epoch;
    assert!(
        (4.0 * 60.0..=7.0 * 60.0).contains(&age),
        "sample should be evaluated ~5 minutes ago, got {age:.0}s"
    );
    assert!(instant_value(&v[0]) > 0.0);
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

/// --start/--end/--step: a 10 minute window at a 5 minute step yields 2-3 points.
#[test]
#[ignore]
fn metrics_query_range_with_end_and_step() {
    if harness::require_creds("metrics_query_range_with_end_and_step").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "metrics",
        "query-range",
        COUNT_ALL,
        "--start",
        "now-10m",
        "--end",
        "now",
        "--step",
        "5m",
        "-o",
        "json",
    ]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["metric", "values"]);
    let points = v[0]["values"].as_array().unwrap();
    assert!(
        (2..=3).contains(&points.len()),
        "expected 2-3 points for 10m / 5m, got {}",
        points.len()
    );
    for point in points {
        assert!(point[0].is_number() && point[1].is_string(), "{point}");
    }
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

/// A wildcard name pattern only returns matching names.
#[test]
#[ignore]
fn metrics_search_name_wildcard_filters_names() {
    if harness::require_creds("metrics_search_name_wildcard_filters_names").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["metrics", "search", "--name", "anthropic_*", "-o", "json"]);
    harness::assert_array_of_strings(&v);
    let names = v.as_array().unwrap();
    assert!(!names.is_empty(), "the team ingests anthropic_* metrics");
    assert!(
        names
            .iter()
            .all(|n| n.as_str().unwrap().starts_with("anthropic_")),
        "{v}"
    );
}

/// Semantic search by description. The Olly knowledge base may not be
/// indexed for the team, so only the row shape is asserted.
#[test]
#[ignore]
fn metrics_search_description() {
    if harness::require_creds("metrics_search_description").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "metrics",
        "search",
        "--description",
        "request latency",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["metric_name", "description", "similarity_score"],
    );
    if v.as_array().unwrap().is_empty() {
        eprintln!("[e2e] metrics search --description returned no rows: knowledge base not indexed for this team");
    }
    harness::run_ok(&[
        "metrics",
        "search",
        "--description",
        "request latency",
        "-o",
        "text",
    ]);
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
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["label"]);
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|l| l["label"] == "__name__"),
        "every metric carries __name__: {v}"
    );

    // Text mode lists each label (regression: the table used to be empty).
    let text = harness::run_ok_nonempty(&["metrics", "get-labels", &metric, "-o", "text"]);
    let text = String::from_utf8_lossy(&text);
    assert!(
        text.contains("__name__"),
        "text table must list the labels: {text}"
    );
}

/// Output formats on one representative metrics command.
#[test]
#[ignore]
fn metrics_query_text_and_toon_output() {
    if harness::require_creds("metrics_query_text_and_toon_output").is_none() {
        return;
    }
    let text = harness::run_ok_nonempty(&["metrics", "query", COUNT_ALL, "-o", "text"]);
    let text = String::from_utf8_lossy(&text);
    assert!(text.contains("Labels") && text.contains("Value"), "{text}");

    let toon = harness::run_ok_nonempty(&["metrics", "query", COUNT_ALL, "-o", "toon"]);
    let toon = String::from_utf8_lossy(&toon);
    assert!(toon.contains("value"), "{toon}");

    let toon = harness::run_ok_nonempty(&[
        "metrics",
        "query-range",
        COUNT_ALL,
        "--start",
        "now-10m",
        "--step",
        "5m",
        "-o",
        "toon",
    ]);
    let toon = String::from_utf8_lossy(&toon);
    assert!(
        toon.contains("T") && toon.contains("Z"),
        "pivoted toon uses ISO timestamps as columns: {toon}"
    );
}

/// Discover a metric name from `metrics search --name 'anthropic_*' -o json`.
/// The name-search rendering emits a top-level array of strings.
fn discover_metric_name() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout =
                harness::run_ok(&["metrics", "search", "--name", "anthropic_*", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?.first()?.as_str().map(String::from)
        })
        .clone()
}
