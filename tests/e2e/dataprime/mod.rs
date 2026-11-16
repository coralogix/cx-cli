//! `cx dataprime`: the embedded language reference (`list` / `show`, no
//! network, so not `#[ignore]`d) and raw `query` against the test team.
//! Queries go through the 429-retrying runners in `logs`.

use crate::harness;
use crate::logs::{query_json, query_output, query_stdout};

// ── list / show (embedded docs, no credentials) ──────────────────────────────

#[test]
fn dataprime_list_all() {
    let v = harness::run_ok_json(&["dataprime", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(
        &v,
        &["name", "type", "syntax", "description", "category"],
    );
    let items = v.as_array().unwrap();
    assert!(items.iter().any(|i| i["type"] == "command"));
    assert!(items.iter().any(|i| i["type"] == "function"));
    assert!(items.iter().any(|i| i["name"] == "filter"));
}

#[test]
fn dataprime_list_filter_and_name() {
    let v = harness::run_ok_json(&[
        "dataprime",
        "list",
        "--filter",
        "commands",
        "--name",
        "filt",
        "-o",
        "json",
    ]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["name", "type"]);
    for item in v.as_array().unwrap() {
        assert_eq!(item["type"], "command", "{item}");
        assert!(
            item["name"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("filt"),
            "{item}"
        );
    }

    let v = harness::run_ok_json(&["dataprime", "list", "--filter", "functions", "-o", "json"]);
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["type"] == "function"));

    let text = harness::run_ok_nonempty(&[
        "dataprime",
        "list",
        "--filter",
        "functions",
        "--name",
        "toInterval",
    ]);
    assert!(String::from_utf8_lossy(&text).contains("toInterval"));

    let text = harness::run_ok_nonempty(&["dataprime", "list", "--name", "cx-no-such-item-xyz"]);
    assert!(String::from_utf8_lossy(&text).contains("No matching items"));
}

#[test]
fn dataprime_show_known_and_unknown() {
    let v = harness::run_ok_json(&["dataprime", "show", "filter", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["name", "type", "syntax", "description", "category"]);
    assert_eq!(v["name"], "filter");
    assert_eq!(v["type"], "command");

    let text = harness::run_ok_nonempty(&["dataprime", "show", "source"]);
    let text = String::from_utf8_lossy(&text);
    assert!(
        text.contains("Syntax:") && text.contains("Description:"),
        "{text}"
    );

    let toon = harness::run_ok_nonempty(&["dataprime", "show", "toInterval", "-o", "toon"]);
    assert!(String::from_utf8_lossy(&toon).contains("toInterval"));

    let output = harness::cx()
        .args(["dataprime", "show", "cx_no_such_thing"])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No command or function named"));
}

// ── query (live) ─────────────────────────────────────────────────────────────

#[test]
#[ignore]
fn dataprime_query_logs() {
    if harness::require_creds("dataprime_query_logs").is_none() {
        return;
    }
    let v = query_json(&[
        "dataprime",
        "query",
        "--source",
        "logs",
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

/// `--source spans` with every time/limit/tier flag, plus a query that
/// names its own source (the flag is then ignored).
#[test]
#[ignore]
fn dataprime_query_spans_with_all_flags() {
    if harness::require_creds("dataprime_query_spans_with_all_flags").is_none() {
        return;
    }
    let v = query_json(&[
        "dataprime",
        "query",
        "filter $l.applicationName == 'otel-demo' | limit 5",
        "--source",
        "spans",
        "--start",
        harness::SHORT_WINDOW_START,
        "--end",
        "now",
        "--limit",
        "2",
        "--tier",
        "archive",
        "--no-strict-fields-validation",
        "-o",
        "json",
    ]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["metadata", "labels", "userData"]);
    assert!(v.as_array().unwrap().len() <= 2);
    assert_eq!(v[0]["labels"]["applicationName"], "otel-demo");

    let v = query_json(&[
        "dataprime",
        "query",
        "source logs | filter $l.applicationname == 'otel-demo' | limit 2",
        "-s",
        "spans",
        "--start",
        harness::SHORT_WINDOW_START,
        "--tier",
        "frequent",
        "-o",
        "json",
    ]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["labels"]);
    assert_eq!(
        v[0]["labels"]["applicationname"], "otel-demo",
        "an explicit `source logs` wins over --source spans: {v}"
    );
}

#[test]
#[ignore]
fn dataprime_strict_validation_and_extended_information() {
    if harness::require_creds("dataprime_strict_validation_and_extended_information").is_none() {
        return;
    }
    let query = "source logs | create missing from $d.cx_cli_missing_field_8b973d6a | limit 1";
    let strict = query_output(&[
        "dataprime",
        "query",
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
        "unknown keypaths must fail with --strict-fields-validation"
    );
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(stderr.contains("keypath does not exist"), "{stderr}");
    assert!(
        stderr.contains("--no-strict-fields-validation"),
        "strict keypath errors must name the flag that turns validation off: {stderr}"
    );
    assert!(
        stderr.contains("strict_fields_validation"),
        "strict keypath errors must name strict_fields_validation in ~/.cx/config.toml: {stderr}"
    );
    let permissive = query_json(&[
        "dataprime",
        "query",
        query,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "1",
        "--extended-information",
        "-o",
        "json",
    ]);
    assert!(permissive["results"].is_array());
    assert!(permissive["queryId"].is_string());
    assert_eq!(permissive["statistics"]["status"], "COMPLETED");
    assert!(permissive["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.to_string().contains("keypath does not exist")));
}

/// Text mode for a raw query pretty-prints each row; toon mode renames keys.
#[test]
#[ignore]
fn dataprime_query_text_and_toon_output() {
    if harness::require_creds("dataprime_query_text_and_toon_output").is_none() {
        return;
    }
    let args = [
        "dataprime",
        "query",
        "filter $l.applicationname == 'otel-demo' | limit 1",
        "--source",
        "logs",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "1",
    ];
    let text = query_stdout(&[&args[..], &["-o", "text"]].concat());
    assert!(text.contains("\"applicationname\": \"otel-demo\""));
    let toon = query_stdout(&[&args[..], &["-o", "toon"]].concat());
    assert!(toon.contains("\"$l\""));
}
