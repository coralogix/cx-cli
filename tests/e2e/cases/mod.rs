use std::sync::OnceLock;

use crate::harness;

// `cx cases list` was removed — case discovery now lives in the cases dataset
// (queried via DataPrime). These read-only sanity checks therefore need a known
// case ID supplied out-of-band via the `CX_TEST_CASE_ID` env var; tests that
// require one skip cleanly when it is absent.

#[test]
#[ignore]
fn cases_get() {
    if harness::require_creds("cases_get").is_none() {
        return;
    }
    let Some(id) = discover_case_id() else {
        eprintln!("[e2e] skipping cases_get: set CX_TEST_CASE_ID to a known case");
        return;
    };
    let v = harness::run_ok_json(&["cases", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["case"]);
}

#[test]
#[ignore]
fn cases_events_list() {
    if harness::require_creds("cases_events_list").is_none() {
        return;
    }
    let Some(id) = discover_case_id() else {
        eprintln!("[e2e] skipping cases_events_list: set CX_TEST_CASE_ID to a known case");
        return;
    };
    let v = harness::run_ok_json(&["cases", "events", "list", &id, "-o", "json"]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn cases_events_get() {
    if harness::require_creds("cases_events_get").is_none() {
        return;
    }
    let Some(event_id) = discover_event_id() else {
        eprintln!("[e2e] skipping cases_events_get: no events on the discovered case");
        return;
    };
    harness::run_ok(&["cases", "events", "get", &event_id, "-o", "json"]);
}

#[test]
#[ignore]
fn cases_notifications() {
    if harness::require_creds("cases_notifications").is_none() {
        return;
    }
    let Some(id) = discover_case_id() else {
        eprintln!("[e2e] skipping cases_notifications: set CX_TEST_CASE_ID to a known case");
        return;
    };
    let v = harness::run_ok_json(&["cases", "notifications", &id, "-o", "json"]);
    harness::assert_array(&v);
}

// NOTE: mutating subcommands (assign/unassign/acknowledge/unacknowledge/resolve/close/
// set-priority/clear-priority/update) are intentionally not exercised in e2e tests
// — they touch shared test-team state. Add coverage only with a paired-undo plan.

/// A known case ID for read-only e2e checks, supplied via `CX_TEST_CASE_ID`.
/// Returns `None` when unset so dependent tests skip cleanly.
fn discover_case_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            std::env::var("CX_TEST_CASE_ID")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .clone()
}

/// The first event ID on the discovered case, used to sanity-check `events get`.
fn discover_event_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if harness::require_creds("cases_events_discover").is_none() {
                return None;
            }
            let id = discover_case_id()?;
            let stdout = harness::run_ok(&["cases", "events", "list", &id, "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}
