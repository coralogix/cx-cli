use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn rule_groups_list() {
    if harness::require_creds("rule_groups_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["rule-groups", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn rule_groups_get() {
    if harness::require_creds("rule_groups_get").is_none() {
        return;
    }
    let id = match discover_rule_group_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping rule_groups_get: no rule groups available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["rule-groups", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn rule_groups_usage_limits() {
    if harness::require_creds("rule_groups_usage_limits").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["rule-groups", "usage-limits", "-o", "json"]);
}

fn discover_rule_group_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("rule_groups_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["rule-groups", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
        .clone()
}
