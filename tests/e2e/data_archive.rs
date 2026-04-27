use crate::harness;

#[test]
#[ignore]
fn data_archive_metrics_get() {
    if harness::require_creds("data_archive_metrics_get").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["data-archive", "metrics", "get", "-o", "json"]);
}

#[test]
#[ignore]
fn data_archive_logs_get() {
    if harness::require_creds("data_archive_logs_get").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["data-archive", "logs", "get", "-o", "json"]);
}
