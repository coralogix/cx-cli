use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn profiling_services() {
    if harness::require_creds("profiling_services").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "profiling",
        "services",
        "--category",
        "cpu",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &["service_name", "last_active", "total_cpu_seconds"],
    );
}

#[test]
#[ignore]
fn profiling_metadata() {
    if harness::require_creds("profiling_metadata").is_none() {
        return;
    }
    let Some(service) = discover_service() else {
        eprintln!("[e2e] skipping profiling_metadata: no profiled services on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "profiling",
        "metadata",
        &service,
        "--start",
        harness::SHORT_WINDOW_START,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["service_name", "profile_types"]);
}

#[test]
#[ignore]
fn profiling_filters() {
    if harness::require_creds("profiling_filters").is_none() {
        return;
    }
    let Some(service) = discover_service() else {
        eprintln!("[e2e] skipping profiling_filters: no profiled services on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "profiling",
        "filters",
        &service,
        "--profile-type",
        "cpu",
        "--start",
        harness::SHORT_WINDOW_START,
        "--max-values-per-key",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["service_name", "profile_type", "filters"]);
    harness::assert_array_of_objects_with_keys(&v["filters"], &["key", "values", "total_values"]);
}

#[test]
#[ignore]
fn profiling_metrics() {
    if harness::require_creds("profiling_metrics").is_none() {
        return;
    }
    let Some(service) = discover_service() else {
        eprintln!("[e2e] skipping profiling_metrics: no profiled services on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "profiling",
        "metrics",
        &service,
        "--profile-type",
        "cpu",
        "--start",
        harness::SHORT_WINDOW_START,
        "--max-buckets",
        "5",
        "--limit",
        "3",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["unit", "sum_by", "total_series", "series"]);
    harness::assert_array_of_objects_with_keys(&v["series"], &["group_by_value", "total"]);
}

#[test]
#[ignore]
fn profiling_flame_graph() {
    if harness::require_creds("profiling_flame_graph").is_none() {
        return;
    }
    let Some(service) = discover_service() else {
        eprintln!("[e2e] skipping profiling_flame_graph: no profiled services on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "profiling",
        "flame-graph",
        &service,
        "--profile-type",
        "cpu",
        "--start",
        harness::SHORT_WINDOW_START,
        "--max-depth",
        "5",
        "--top-functions",
        "5",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["total_value", "top_functions", "tree"]);
    harness::assert_array_of_objects_with_keys(&v["top_functions"], &["function", "self"]);
}

fn discover_service() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("profiling_discover_service")?;
            let stdout = harness::run_ok(&[
                "profiling",
                "services",
                "--category",
                "cpu",
                "--start",
                harness::SHORT_WINDOW_START,
                "--limit",
                "1",
                "-o",
                "json",
            ]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()?
                .get("service_name")?
                .as_str()
                .map(String::from)
        })
        .clone()
}
