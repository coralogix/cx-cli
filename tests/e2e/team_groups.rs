use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn team_groups_list() {
    if harness::require_creds("team_groups_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["team-groups", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["group_id", "name"]);
}

#[test]
#[ignore]
fn team_groups_get() {
    if harness::require_creds("team_groups_get").is_none() {
        return;
    }
    let id = match discover_team_group_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping team_groups_get: no team groups available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["team-groups", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["group_id", "name"]);
}

fn discover_team_group_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("team_groups_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["team-groups", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            // Try flat array with "group_id"
            if let Some(arr) = v.as_array() {
                if let Some(id) = arr
                    .first()
                    .and_then(|item| item.get("group_id"))
                    .map(|x| x.to_string().trim_matches('"').to_string())
                {
                    return Some(id);
                }
            }
            // Try wrapped "groups" array with "groupId"
            v.get("groups")
                .and_then(|g| g.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| {
                    item.get("groupId")
                        .or_else(|| item.get("group_id"))
                        .map(|x| x.to_string().trim_matches('"').to_string())
                })
        })
        .clone()
}
