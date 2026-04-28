use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn alerts_list() {
    if harness::require_creds("alerts_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["alerts", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn alerts_get() {
    if harness::require_creds("alerts_get").is_none() {
        return;
    }
    let id = match discover_alert_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping alerts_get: no alerts available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["alerts", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["alertDef"]);
}

fn discover_alert_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("alerts_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["alerts", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
        .clone()
}
