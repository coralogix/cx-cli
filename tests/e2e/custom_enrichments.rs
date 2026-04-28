use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn custom_enrichments_list() {
    if harness::require_creds("custom_enrichments_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["custom-enrichments", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn custom_enrichments_get() {
    if harness::require_creds("custom_enrichments_get").is_none() {
        return;
    }
    let id = match discover_custom_enrichment_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping custom_enrichments_get: no custom enrichments on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["custom-enrichments", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

fn discover_custom_enrichment_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("custom_enrichments_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["custom-enrichments", "list", "-o", "json"]);
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
