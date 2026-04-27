use crate::harness;

#[test]
#[ignore]
fn webhooks_list() {
    if harness::require_creds("webhooks_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["webhooks", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn webhooks_types() {
    if harness::require_creds("webhooks_types").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["webhooks", "types", "-o", "json"]);
}
