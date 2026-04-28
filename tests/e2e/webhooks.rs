use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn webhooks_list() {
    if harness::require_creds("webhooks_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["webhooks", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn webhooks_get() {
    if harness::require_creds("webhooks_get").is_none() {
        return;
    }
    let id = match discover_webhook_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping webhooks_get: no webhooks available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["webhooks", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn webhooks_types() {
    if harness::require_creds("webhooks_types").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["webhooks", "types", "-o", "json"]);
}

fn discover_webhook_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("webhooks_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["webhooks", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
        .clone()
}
