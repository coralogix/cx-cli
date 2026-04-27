use crate::harness;

#[test]
#[ignore]
fn tco_policies_list() {
    if harness::require_creds("tco_policies_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["tco-policies", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn tco_policies_settings() {
    if harness::require_creds("tco_policies_settings").is_none() {
        return;
    }
    // Settings endpoint returns a JSON object; just assert it succeeds
    harness::run_ok_nonempty(&["tco-policies", "settings", "-o", "json"]);
}
