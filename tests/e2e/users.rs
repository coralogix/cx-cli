use crate::harness;

#[test]
#[ignore]
fn users_search() {
    if harness::require_creds("users_search").is_none() {
        return;
    }
    // Users search may return empty if no team_id resolves, so just verify
    // we get valid JSON back.
    let _v = harness::run_ok_json(&["iam", "users", "search", "-o", "json"]);
}
