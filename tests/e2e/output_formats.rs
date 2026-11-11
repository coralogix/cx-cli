//! Smoke tests that exercise the three output formats on representative
//! read-only commands, plus the spill path: when a non-aggregated DataPrime
//! result exceeds `max_dataprime_direct_output_size` in toon mode, cx writes
//! it to `temp_dir` and prints the path instead.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::harness;
use crate::logs::{query_json, query_stdout, run_with_429_retry};
use crate::profiles_and_local::{cx_in_home_with_creds, temp_home};

#[test]
#[ignore]
fn alerts_list_text() {
    if harness::require_creds("alerts_list_text").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["alerts", "list", "-o", "text"]);
}

#[test]
#[ignore]
fn alerts_list_json() {
    if harness::require_creds("alerts_list_json").is_none() {
        return;
    }
    harness::run_ok_json(&["alerts", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn alerts_list_agents() {
    if harness::require_creds("alerts_list_agents").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["alerts", "list", "-o", "toon"]);
}

/// The same logs query in all three formats.
#[test]
#[ignore]
fn logs_text_json_toon() {
    if harness::require_creds("logs_text_json_toon").is_none() {
        return;
    }
    let base = [
        "logs",
        "filter $l.applicationname == 'otel-demo' | limit 2",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "2",
    ];
    let json = query_json(&[&base[..], &["-o", "json"]].concat());
    harness::assert_nonempty_array_of_objects_with_keys(&json, &["metadata", "labels", "userData"]);
    let text = query_stdout(&[&base[..], &["-o", "text"]].concat());
    assert!(text.contains('['));
    let toon = query_stdout(&[&base[..], &["-o", "toon"]].concat());
    assert!(toon.contains("\"$d\""));
}

/// Writes a config with a 1-byte direct-output limit so any row spills.
fn spill_home(tag: &str) -> (PathBuf, PathBuf) {
    let home = temp_home(tag);
    let spill_dir = home.join("spill");
    fs::create_dir_all(&spill_dir).unwrap();
    fs::create_dir_all(home.join(".cx")).unwrap();
    fs::write(
        home.join(".cx").join("config.toml"),
        format!(
            "max_dataprime_direct_output_size = 1\ntemp_dir = \"{}/\"\n",
            spill_dir.display()
        ),
    )
    .unwrap();
    (home, spill_dir)
}

/// Parses `N results retrieved. Results written to: <path>`.
fn parse_spill_line(stdout: &str) -> (usize, PathBuf) {
    let line = stdout
        .lines()
        .find(|l| l.contains("Results written to:"))
        .unwrap_or_else(|| panic!("no spill line in stdout:\n{stdout}"));
    let count: usize = line
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("spill line starts with the row count: {line}"));
    let path = line.split("Results written to:").nth(1).unwrap().trim();
    (count, Path::new(path).to_path_buf())
}

/// Runs cx inside `home` (env-only credentials), retrying on a 429.
fn run_in(home: &Path, args: &[&str]) -> (bool, String, String) {
    let output = run_with_429_retry(|| {
        let mut cmd = cx_in_home_with_creds(home);
        cmd.args(args);
        cmd
    });
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// toon mode spills to a `cx_results_*.json` file under `temp_dir`, prints
/// its path, and the file holds the toon-shaped rows. JSON mode never spills.
#[test]
#[ignore]
fn logs_toon_spills_to_temp_dir_when_over_limit() {
    if harness::require_creds("logs_toon_spills_to_temp_dir_when_over_limit").is_none() {
        return;
    }
    let (home, spill_dir) = spill_home("spill");
    let query = [
        "logs",
        "filter $l.applicationname == 'otel-demo' | limit 3",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "3",
    ];

    let (ok, out, err) = run_in(&home, &[&query[..], &["-o", "toon"]].concat());
    assert!(ok, "spilled query must still exit 0: {err}");
    let (count, path) = parse_spill_line(&out);
    assert!((1..=3).contains(&count), "{out}");
    assert!(path.is_file(), "spill file must exist: {}", path.display());
    assert_eq!(
        path.parent(),
        Some(spill_dir.as_path()),
        "spill goes to temp_dir"
    );
    assert!(path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("cx_results_"));
    let rows: Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).expect("spill file is JSON");
    let rows = rows.as_array().expect("spill file is a JSON array");
    assert_eq!(rows.len(), count);
    assert!(
        rows[0].get("$l").is_some() && rows[0].get("$d").is_some(),
        "{}",
        rows[0]
    );
    assert!(
        rows[0].get("metadata").is_none(),
        "toon rows are renamed: {}",
        rows[0]
    );

    // --extended-information spills the whole envelope.
    let (ok, out, err) = run_in(
        &home,
        &[&query[..], &["--extended-information", "-o", "toon"]].concat(),
    );
    assert!(ok, "{err}");
    let (_, path) = parse_spill_line(&out);
    let envelope: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    harness::assert_object_with_keys(&envelope, &["results", "queryId", "warnings", "statistics"]);

    // JSON output ignores the limit and prints the rows inline.
    let (ok, out, err) = run_in(&home, &[&query[..], &["-o", "json"]].concat());
    assert!(ok, "{err}");
    assert!(!out.contains("Results written to:"), "{out}");
    let inline: Value = serde_json::from_str(&out).expect("json stays inline");
    assert!(!inline.as_array().unwrap().is_empty());

    // Aggregations never spill in toon mode either.
    let (ok, out, err) = run_in(
        &home,
        &[
            "logs",
            "filter $l.applicationname == 'otel-demo' | groupby $l.subsystemname aggregate count() as n",
            "--start",
            harness::SHORT_WINDOW_START,
            "-o",
            "toon",
        ],
    );
    assert!(ok, "{err}");
    assert!(
        !out.contains("Results written to:") && out.contains("subsystemname"),
        "{out}"
    );

    // A fresh spill file is kept by cleanup; once stale it is removed.
    let (ok, out, _) = run_in(&home, &["cleanup"]);
    assert!(ok);
    assert!(out.contains("No stale result files found"), "{out}");
    let stale = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60);
    for entry in fs::read_dir(&spill_dir).unwrap() {
        let file = fs::File::options()
            .write(true)
            .open(entry.unwrap().path())
            .unwrap();
        file.set_modified(stale).unwrap();
    }
    let (ok, out, _) = run_in(&home, &["cleanup"]);
    assert!(ok);
    assert!(out.contains("Removed 2 stale result file"), "{out}");
    assert!(fs::read_dir(&spill_dir).unwrap().next().is_none());
}

/// Spans spill the same way.
#[test]
#[ignore]
fn spans_toon_spills_to_temp_dir_when_over_limit() {
    if harness::require_creds("spans_toon_spills_to_temp_dir_when_over_limit").is_none() {
        return;
    }
    let (home, spill_dir) = spill_home("spill_spans");
    let (ok, out, err) = run_in(
        &home,
        &[
            "spans",
            "limit 2",
            "--start",
            harness::SHORT_WINDOW_START,
            "--limit",
            "2",
            "-o",
            "toon",
        ],
    );
    assert!(ok, "{err}");
    let (count, path) = parse_spill_line(&out);
    assert!(count >= 1);
    assert!(
        path.starts_with(&spill_dir) && path.is_file(),
        "{}",
        path.display()
    );
}
