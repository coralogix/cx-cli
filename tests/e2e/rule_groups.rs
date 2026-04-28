use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn rule_groups_list() {
    if harness::require_creds("rule_groups_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["rules", "list", "-o", "json"]);
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
    let v = harness::run_ok_json(&["rules", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

fn discover_rule_group_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("rule_groups_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["rules", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}
