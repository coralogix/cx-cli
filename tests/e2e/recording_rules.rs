use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn recording_rules_list() {
    if harness::require_creds("recording_rules_list").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["recording-rules", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn recording_rules_get() {
    if harness::require_creds("recording_rules_get").is_none() {
        return;
    }
    let id = match discover_recording_rule_id() {
        Some(id) => id,
        None => {
            eprintln!(
                "[e2e] skipping recording_rules_get: no recording rules available on test team"
            );
            return;
        }
    };
    let v = harness::run_ok_json(&["recording-rules", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

fn discover_recording_rule_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("recording_rules_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["recording-rules", "list", "-o", "json"]);
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
