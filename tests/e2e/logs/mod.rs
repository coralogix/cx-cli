//! `cx logs` against the live otel-demo stream on the test team.
//!
//! DataPrime string literals are single-quoted (`'otel-demo'`); a
//! double-quoted literal is a compilation error on the API.
//!
//! Also home of the shared DataPrime query runners: the query endpoint
//! rate-limits bursts (HTTP 429, "Retry after N seconds"), which parallel
//! test threads and parallel CI jobs trigger. The runners wait the announced
//! time and retry, so a throttled run does not fail.

use std::process::Output;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

use crate::harness;

/// The otel-demo stack ships logs continuously, so the last 15 minutes always
/// hold rows for this filter.
const OTEL_FILTER: &str = "filter $l.applicationname == 'otel-demo' | limit 5";

const MAX_ATTEMPTS: usize = 3;

/// Runs the command `build` produces, retrying on a 429 from the query API.
pub(crate) fn run_with_429_retry(build: impl Fn() -> Command) -> Output {
    let mut output = build().output().expect("cx must run");
    for _ in 1..MAX_ATTEMPTS {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if output.status.success() || !stderr.contains("Rate limited") {
            break;
        }
        let secs = stderr
            .split("Retry after ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(60)
            .min(120);
        eprintln!("[e2e] query rate limited; retrying in {secs}s");
        std::thread::sleep(Duration::from_secs(secs + 1));
        output = build().output().expect("cx must run");
    }
    output
}

/// `cx <args>` with the 429 retry; returns the raw output (any exit code).
pub(crate) fn query_output(args: &[&str]) -> Output {
    run_with_429_retry(|| {
        let mut cmd = harness::cx();
        cmd.args(args);
        cmd
    })
}

/// `harness::run_ok_json` with the 429 retry.
pub(crate) fn query_json(args: &[&str]) -> Value {
    let output = query_output(args);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cx {} failed: {stderr}",
        args.join(" ")
    );
    assert!(!stderr.contains("API request failed"), "{stderr}");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "expected valid JSON from `cx {}`: {e}\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// `harness::run_ok_nonempty` with the 429 retry; returns stdout as text.
pub(crate) fn query_stdout(args: &[&str]) -> String {
    let output = query_output(args);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cx {} failed: {stderr}",
        args.join(" ")
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        !stdout.is_empty(),
        "expected non-empty stdout from `cx {}`",
        args.join(" ")
    );
    stdout
}

#[test]
#[ignore]
fn logs_basic() {
    if harness::require_creds("logs_basic").is_none() {
        return;
    }
    let v = query_json(&[
        "logs",
        "limit 1",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        harness::SMALL_LIMIT,
        "-o",
        "json",
    ]);
    // Row shape varies by query (aggregates, projections); just verify the
    // top-level container is the expected array of rows.
    harness::assert_array(&v);
}

/// --start/--end/--limit plus a label filter: every row carries the label we
/// filtered on, and --limit caps the row count.
#[test]
#[ignore]
fn logs_otel_demo_filter_returns_labelled_rows() {
    if harness::require_creds("logs_otel_demo_filter_returns_labelled_rows").is_none() {
        return;
    }
    let v = query_json(&[
        "logs",
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
        assert_eq!(
            row["labels"]["applicationname"], "otel-demo",
            "every row must match the filter: {row}"
        );
        assert!(row["metadata"]["timestamp"].is_string(), "{row}");
    }
}

/// Both storage tiers answer the same query.
#[test]
#[ignore]
fn logs_tier_frequent_and_archive() {
    if harness::require_creds("logs_tier_frequent_and_archive").is_none() {
        return;
    }
    for tier in ["frequent", "archive"] {
        let v = query_json(&[
            "logs",
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

/// An aggregation returns flat rows (no metadata/labels/userData envelope).
#[test]
#[ignore]
fn logs_aggregation_rows_are_flat() {
    if harness::require_creds("logs_aggregation_rows_are_flat").is_none() {
        return;
    }
    let v = query_json(&[
        "logs",
        "filter $l.applicationname == 'otel-demo' | groupby $l.subsystemname aggregate count() as n",
        "--start",
        harness::SHORT_WINDOW_START,
        "-o",
        "json",
    ]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["n", "subsystemname"]);
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["n"].as_u64().unwrap_or(0) > 0));
}

/// --strict-fields-validation rejects an unknown keypath; the opposite flag
/// lets it through; --extended-information wraps the rows in an envelope.
#[test]
#[ignore]
fn logs_strict_fields_validation_flags() {
    if harness::require_creds("logs_strict_fields_validation_flags").is_none() {
        return;
    }
    let query = "create missing from $d.cx_cli_missing_field_8b973d6a | limit 1";
    let strict = query_output(&[
        "logs",
        query,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "1",
        "--strict-fields-validation",
        "-o",
        "json",
    ]);
    assert!(
        !strict.status.success(),
        "unknown keypath must fail under strict validation"
    );
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(stderr.contains("keypath does not exist"), "{stderr}");
    assert!(stderr.contains("--no-strict-fields-validation"), "{stderr}");

    let permissive = query_json(&[
        "logs",
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
    assert!(permissive["results"].is_array());
    assert_eq!(permissive["statistics"]["status"], "COMPLETED");
}

/// Text mode prints one `<timestamp> [<severity>] <message>` line per row;
/// toon mode renames the envelope keys to `$m` / `$l` / `$d`.
#[test]
#[ignore]
fn logs_text_and_toon_output() {
    if harness::require_creds("logs_text_and_toon_output").is_none() {
        return;
    }
    let text = query_stdout(&[
        "logs",
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
    assert!(
        lines.iter().all(|l| l.contains('[') && l.contains(']')),
        "each text row carries a [severity] tag: {text}"
    );

    let toon = query_stdout(&[
        "logs",
        OTEL_FILTER,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "2",
        "-o",
        "toon",
    ]);
    assert!(
        toon.contains("\"$l\"") && toon.contains("\"$d\"") && toon.contains("\"$m\""),
        "toon rows use the $m/$l/$d keys: {toon}"
    );
    assert!(toon.contains("otel-demo"));
}
