use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn alerts_list() {
    if harness::require_creds("alerts_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["alerts", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "enabled"]);
}

#[test]
#[ignore]
fn alerts_get() {
    if harness::require_creds("alerts_get").is_none() {
        return;
    }
    let Some(id) = discover_alert_id() else {
        eprintln!("[e2e] skipping alerts_get: no alerts available on test team");
        return;
    };
    let v = harness::run_ok_json(&["alerts", "get", &id, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["alertDef"]);
}

/// Discover an alert id from `alerts list -o json`. The list rendering emits
/// a top-level array of alert objects with an `id` field.
fn discover_alert_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["alerts", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}

// Mutating alert commands (`create`, `enable`, `disable`) are deliberately
// not covered yet. `create` has no companion delete; `enable`/`disable`
// would change shared test team state. Revisit when we're comfortable
// mutating the test team from CI.
