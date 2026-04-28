use crate::harness;

#[test]
#[ignore]
fn data_usage_summary() {
    if harness::require_creds("data_usage_summary").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["data-usage", "summary", "-o", "json"]);
}

#[test]
#[ignore]
fn data_usage_daily() {
    if harness::require_creds("data_usage_daily").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&[
        "data-usage",
        "daily",
        "--type",
        "processed-gbs",
        "--start",
        "now-7d",
        "--end",
        "now",
        "-o",
        "json",
    ]);
}
