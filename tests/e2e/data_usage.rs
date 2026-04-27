use crate::harness;

#[test]
#[ignore]
fn data_usage_summary() {
    if harness::require_creds("data_usage_summary").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["data-usage", "summary", "-o", "json"]);
}
