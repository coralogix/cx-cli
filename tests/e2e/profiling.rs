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

/// A service name no one profiles. The API answers every per-service command
/// with an empty-but-well-formed document, so the full flag surface can run
/// on a team without profiling data.
const NO_SUCH_SERVICE: &str = "cx-e2e-no-such-service";

/// `services` with the memory ranking and the paging/environment flags. The
/// test team has no profiled services, so an empty list is a valid answer.
#[test]
#[ignore]
fn profiling_services_memory_with_paging_and_environment() {
    if harness::require_creds("profiling_services_memory_with_paging_and_environment").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "profiling",
        "services",
        "--category",
        "memory",
        "--start",
        "now-24h",
        "--end",
        "now",
        "--environment",
        "staging",
        "--environment",
        "production",
        "--limit",
        "2",
        "--offset",
        "1",
        "--include-time-series",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(
        &v,
        &[
            "service_name",
            "last_active",
            "total_memory_allocations_bytes",
        ],
    );
    assert!(v.as_array().unwrap().len() <= 2);
    if v.as_array().unwrap().is_empty() {
        eprintln!("[e2e] profiling services: no profiled services on test team");
    }
    harness::run_ok(&[
        "profiling",
        "services",
        "--category",
        "memory",
        "-o",
        "text",
    ]);
    harness::run_ok(&["profiling", "services", "--category", "cpu", "-o", "toon"]);

    for (args, expected) in [
        (vec!["--category", "disk"], "category"),
        (
            vec!["--category", "cpu", "--limit", "0"],
            "--limit must be between",
        ),
        (
            vec!["--category", "cpu", "--limit", "101"],
            "--limit must be between",
        ),
    ] {
        let output = harness::cx()
            .args(["profiling", "services"])
            .args(&args)
            .output()
            .expect("cx must run");
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.to_lowercase().contains(expected),
            "{args:?}: {stderr}"
        );
    }
}

#[test]
#[ignore]
fn profiling_metadata_unprofiled_service_lists_every_profile_type() {
    if harness::require_creds("profiling_metadata_unprofiled_service_lists_every_profile_type")
        .is_none()
    {
        return;
    }
    let v = harness::run_ok_json(&[
        "profiling",
        "metadata",
        NO_SUCH_SERVICE,
        "--start",
        "now-1h",
        "--end",
        "now",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(
        &v,
        &[
            "service_name",
            "language",
            "profile_types",
            "profile_types_with_data",
        ],
    );
    assert_eq!(v["service_name"], NO_SUCH_SERVICE);
    harness::assert_nonempty_array_of_objects_with_keys(
        &v["profile_types"],
        &["profile_type", "has_data"],
    );
    assert!(v["profile_types"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["has_data"] == false));
    assert_eq!(v["profile_types_with_data"], serde_json::json!([]));
    harness::run_ok_nonempty(&["profiling", "metadata", NO_SUCH_SERVICE, "-o", "text"]);
}

/// `filters` with every filter syntax and `--max-values-per-key`.
#[test]
#[ignore]
fn profiling_filters_with_filter_syntaxes() {
    if harness::require_creds("profiling_filters_with_filter_syntaxes").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "profiling",
        "filters",
        NO_SUCH_SERVICE,
        "--profile-type",
        "cpu",
        "--start",
        "now-1h",
        "--end",
        "now",
        "--filter",
        "k8s.pod.name:exists",
        "--filter",
        "deployment.environment.name=staging,production",
        "--filter",
        "service.version!=0.0.0",
        "--filter",
        "host.name:not-exists",
        "--max-values-per-key",
        "3",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["service_name", "profile_type", "filters"]);
    assert_eq!(v["profile_type"], "CPU");
    harness::assert_array_of_objects_with_keys(&v["filters"], &["key", "values", "total_values"]);

    let output = harness::cx()
        .args([
            "profiling",
            "filters",
            NO_SUCH_SERVICE,
            "--profile-type",
            "cpu",
            "--max-values-per-key",
            "0",
        ])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--max-values-per-key must be > 0"));
}

/// `metrics` with `--sum-by`, `--filter`, `--max-buckets`, `--limit`, and a
/// memory profile type.
#[test]
#[ignore]
fn profiling_metrics_sum_by_and_filter() {
    if harness::require_creds("profiling_metrics_sum_by_and_filter").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "profiling",
        "metrics",
        NO_SUCH_SERVICE,
        "--profile-type",
        "memory-heap-bytes",
        "--start",
        "now-1h",
        "--end",
        "now",
        "--filter",
        "k8s.pod.name:exists",
        "--sum-by",
        "k8s.pod.name",
        "--max-buckets",
        "3",
        "--limit",
        "2",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["unit", "sum_by", "total_series", "series"]);
    assert_eq!(v["sum_by"], "k8s.pod.name");
    assert_eq!(v["unit"], "bytes");
    harness::assert_array_of_objects_with_keys(&v["series"], &["group_by_value", "total"]);
    assert!(v["series"].as_array().unwrap().len() <= 2);
    harness::run_ok(&[
        "profiling",
        "metrics",
        NO_SUCH_SERVICE,
        "--profile-type",
        "wall",
        "--sum-by",
        "pod",
        "-o",
        "text",
    ]);
}

/// `flame-graph` with `--show-kernel`, `--min-node-ratio`, `--max-depth`,
/// `--top-functions`, and `--filter`; the pruning options echo back.
#[test]
#[ignore]
fn profiling_flame_graph_with_pruning_flags() {
    if harness::require_creds("profiling_flame_graph_with_pruning_flags").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "profiling",
        "flame-graph",
        NO_SUCH_SERVICE,
        "--profile-type",
        "cpu",
        "--start",
        "now-1h",
        "--end",
        "now",
        "--filter",
        "k8s.pod.name:exists",
        "--show-kernel",
        "--min-node-ratio",
        "0.05",
        "--max-depth",
        "3",
        "--top-functions",
        "2",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(
        &v,
        &[
            "total_value",
            "top_functions",
            "tree",
            "min_node_ratio",
            "max_depth",
            "was_seen",
        ],
    );
    assert_eq!(v["min_node_ratio"], 0.05);
    assert_eq!(v["max_depth"], 3);
    assert!(v["top_functions"].as_array().unwrap().len() <= 2);
    harness::assert_object_with_keys(&v["tree"], &["function", "self", "total", "children"]);
    harness::run_ok_nonempty(&[
        "profiling",
        "flame-graph",
        NO_SUCH_SERVICE,
        "--profile-type",
        "cpu",
        "-o",
        "text",
    ]);

    let output = harness::cx()
        .args([
            "profiling",
            "flame-graph",
            NO_SUCH_SERVICE,
            "--profile-type",
            "cpu",
            "--min-node-ratio",
            "1.5",
        ])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--min-node-ratio"));
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
                .iter()
                .find(|item| !harness::is_e2e_item(item))?
                .get("service_name")?
                .as_str()
                .map(String::from)
        })
        .clone()
}
