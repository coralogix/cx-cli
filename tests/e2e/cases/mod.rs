use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn cases_list() {
    if harness::require_creds("cases_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["cases", "list", "-o", "json"]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn cases_get() {
    if harness::require_creds("cases_get").is_none() {
        return;
    }
    let Some(id) = discover_case_id() else {
        eprintln!("[e2e] skipping cases_get: no cases available on test team");
        return;
    };
    let v = harness::run_ok_json(&["cases", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["case"]);
}

#[test]
#[ignore]
fn cases_grouping_keys() {
    if harness::require_creds("cases_grouping_keys").is_none() {
        return;
    }
    harness::run_ok(&["cases", "grouping-keys", "-o", "json"]);
}

#[test]
#[ignore]
fn cases_events_list() {
    if harness::require_creds("cases_events_list").is_none() {
        return;
    }
    let Some(id) = discover_case_id() else {
        eprintln!("[e2e] skipping cases_events_list: no cases available on test team");
        return;
    };
    let v = harness::run_ok_json(&["cases", "events", "list", &id, "-o", "json"]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn cases_notifications() {
    if harness::require_creds("cases_notifications").is_none() {
        return;
    }
    let Some(id) = discover_case_id() else {
        eprintln!("[e2e] skipping cases_notifications: no cases available on test team");
        return;
    };
    let v = harness::run_ok_json(&["cases", "notifications", &id, "-o", "json"]);
    harness::assert_array(&v);
}

// NOTE: mutating subcommands (assign/unassign/acknowledge/unacknowledge/resolve/close/
// set-priority/clear-priority/update) are intentionally not exercised in e2e tests
// — they touch shared test-team state. Add coverage only with a paired-undo plan.

fn discover_case_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("cases_discover").is_none() {
                return None;
            }
            let stdout = harness::run_ok(&["cases", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}
