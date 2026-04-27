use crate::harness;

#[test]
#[ignore]
fn connectors_list() {
    if harness::require_creds("connectors_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["connectors", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn connectors_types() {
    if harness::require_creds("connectors_types").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["connectors", "types", "-o", "json"]);
}
