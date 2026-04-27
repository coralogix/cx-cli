use crate::harness;

#[test]
#[ignore]
fn team_groups_list() {
    if harness::require_creds("team_groups_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["team-groups", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["group_id", "name"]);
}
