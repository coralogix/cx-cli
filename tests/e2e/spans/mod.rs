//! `cx spans` against the live otel-demo traces on the test team.
//!
//! Span labels are camelCase (`$l.applicationName`), unlike logs. DataPrime
//! string literals are single-quoted. Queries go through the 429-retrying
//! runners in `logs` (the query endpoint rate-limits bursts).

use crate::harness;
use crate::logs::{query_json, query_output, query_stdout};

const OTEL_FILTER: &str = "filter $l.applicationName == 'otel-demo' | limit 5";

#[test]
#[ignore]
fn spans_basic() {
    if harness::require_creds("spans_basic").is_none() {
        return;
    }
    let v = query_json(&[
        "spans",
        "limit 1",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        harness::SMALL_LIMIT,
        "-o",
        "json",
    ]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn spans_otel_demo_filter_returns_labelled_rows() {
    if harness::require_creds("spans_otel_demo_filter_returns_labelled_rows").is_none() {
        return;
    }
    let v = query_json(&[
        "spans",
        OTEL_FILTER,
        "--start",
        harness::SHORT_WINDOW_START,
        "--end",
        "now",
        "--limit",
        "3",
        "-o",
        "json",
    ]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["metadata", "labels", "userData"]);
    let rows = v.as_array().unwrap();
    assert!(
        rows.len() <= 3,
        "--limit 3 must cap the rows, got {}",
        rows.len()
    );
    for row in rows {
        assert_eq!(row["labels"]["applicationName"], "otel-demo", "{row}");
        assert!(row["labels"]["serviceName"].is_string(), "{row}");
        assert!(row["metadata"]["duration"].is_string(), "{row}");
    }
}

#[test]
#[ignore]
fn spans_tier_frequent_and_archive() {
    if harness::require_creds("spans_tier_frequent_and_archive").is_none() {
        return;
    }
    for tier in ["frequent", "archive"] {
        let v = query_json(&[
            "spans",
            OTEL_FILTER,
            "--start",
            harness::SHORT_WINDOW_START,
            "--limit",
            "2",
            "--tier",
            tier,
            "-o",
            "json",
        ]);
        harness::assert_nonempty_array_of_objects_with_keys(&v, &["labels"]);
    }
}

#[test]
#[ignore]
fn spans_strict_fields_validation_flags() {
    if harness::require_creds("spans_strict_fields_validation_flags").is_none() {
        return;
    }
    let query = "create missing from $d.cx_cli_missing_field_8b973d6a | limit 1";
    let strict = query_output(&[
        "spans",
        query,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "1",
        "--strict-fields-validation",
        "-o",
        "json",
    ]);
    assert!(!strict.status.success());
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(stderr.contains("keypath does not exist"), "{stderr}");
    assert!(stderr.contains("--no-strict-fields-validation"), "{stderr}");

    let permissive = query_json(&[
        "spans",
        query,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "1",
        "--no-strict-fields-validation",
        "--extended-information",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(
        &permissive,
        &["results", "queryId", "warnings", "statistics"],
    );
    assert_eq!(permissive["statistics"]["status"], "COMPLETED");
    assert!(permissive["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.to_string().contains("keypath does not exist")));
}

/// Text mode prints `<traceId> <spanId> <service> <operation> ...` per span;
/// toon mode renames the envelope keys.
#[test]
#[ignore]
fn spans_text_and_toon_output() {
    if harness::require_creds("spans_text_and_toon_output").is_none() {
        return;
    }
    let text = query_stdout(&[
        "spans",
        OTEL_FILTER,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "2",
        "-o",
        "text",
    ]);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty() && lines.len() <= 2, "{text}");

    let toon = query_stdout(&[
        "spans",
        OTEL_FILTER,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "2",
        "-o",
        "toon",
    ]);
    assert!(
        toon.contains("\"$l\"") && toon.contains("applicationName"),
        "{toon}"
    );
}
