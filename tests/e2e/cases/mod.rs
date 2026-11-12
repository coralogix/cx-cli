use std::sync::OnceLock;

use crate::harness;
use serde_json::Value;

// There is no `cx cases create`: cases are opened by the platform when alerts
// fire, and the writable e2e team currently has none (`POST /cases/cases/v1`
// returns an empty list). The read-only and write round-trips below therefore
// need a known case id supplied out-of-band via `CX_TEST_CASE_ID` and skip
// cleanly when it is absent. `cases_commands_reject_unknown_case` always runs
// and proves every verb reaches the API and fails cleanly on a 404.

const UNKNOWN_CASE: &str = "00000000-0000-0000-0000-000000000000";

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
    harness::run_ok_nonempty(&["cases", "get", &id]);
    harness::run_ok_nonempty(&["cases", "get", &id, "-o", "toon"]);
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

/// Every verb must reach the API and surface its 404 for an unknown case.
/// This is the only cases coverage that runs without a seeded case.
#[test]
#[ignore]
fn cases_commands_reject_unknown_case() {
    if harness::require_creds("cases_commands_reject_unknown_case").is_none() {
        return;
    }
    let read_only: &[&[&str]] = &[
        &["cases", "get", UNKNOWN_CASE],
        &["cases", "events", "list", UNKNOWN_CASE],
        &["cases", "events", "get", UNKNOWN_CASE],
        &["cases", "notifications", UNKNOWN_CASE],
    ];
    for args in read_only {
        let stderr = harness::run_fail(args);
        assert!(stderr.contains("(404)"), "cx {}: {stderr}", args.join(" "));
    }

    if harness::require_write_creds("cases_commands_reject_unknown_case").is_none() {
        return;
    }
    // Writes on a non-existent case cannot change anything; `--yes` only
    // skips the prompt. Resolve/close are included for the same reason.
    let writes: &[&[&str]] = &[
        &["cases", "update", UNKNOWN_CASE, "--title", "cx-e2e"],
        &[
            "cases",
            "update",
            UNKNOWN_CASE,
            "--resolution-reason",
            "cx-e2e",
        ],
        &["cases", "comment", UNKNOWN_CASE, "--text", "cx-e2e"],
        &["cases", "unassign", UNKNOWN_CASE],
        &["cases", "acknowledge", UNKNOWN_CASE],
        &["cases", "unacknowledge", UNKNOWN_CASE],
        &["cases", "set-priority", UNKNOWN_CASE, "--priority", "P5"],
        &["cases", "clear-priority", UNKNOWN_CASE],
        &["cases", "resolve", UNKNOWN_CASE, "--no-reason"],
        &["cases", "close", UNKNOWN_CASE],
    ];
    for args in writes {
        let mut full = args.to_vec();
        full.push("--yes");
        let stderr = harness::run_fail(&full);
        assert!(stderr.contains("(404)"), "cx {}: {stderr}", args.join(" "));
    }
    // `assign` resolves the e-mail against the team directory before the
    // case lookup, so it needs a real teammate to reach the 404.
    if let Some(user) = discover_assignee() {
        let stderr =
            harness::run_fail(&["cases", "assign", UNKNOWN_CASE, "--user", &user, "--yes"]);
        assert!(stderr.contains("(404)"), "cx cases assign: {stderr}");
    } else {
        eprintln!(
            "[e2e] not covered: cases assign — no teammate e-mail found via `iam users search`"
        );
    }
}

/// Paired-undo write round-trip on a seeded case. Every change is reverted,
/// and the case is never resolved or closed (those have no undo).
#[test]
#[ignore]
fn cases_write_roundtrip() {
    if harness::require_write_creds("cases_write_roundtrip").is_none() {
        return;
    }
    let Some(id) = discover_case_id() else {
        eprintln!(
            "[e2e] not covered: cases update/comment/assign/unassign/acknowledge/unacknowledge/\
             set-priority/clear-priority (success path) — no case on the team; set \
             CX_TEST_CASE_ID to a case id to enable the paired-undo round-trip"
        );
        return;
    };
    let before = case_of(&harness::run_ok_json(&["cases", "get", &id, "-o", "json"]));
    let original_title = before["title"].as_str().unwrap_or_default().to_string();
    let was_acknowledged = !before["acknowledgeTime"].is_null();
    let original_override = before
        .pointer("/priorityDetails/override")
        .and_then(Value::as_str)
        .map(String::from);
    let original_assignee = before
        .pointer("/assignee/userId")
        .and_then(Value::as_str)
        .map(String::from);

    // acknowledge <-> unacknowledge, ending in the original state
    let (first, second) = if was_acknowledged {
        ("unacknowledge", "acknowledge")
    } else {
        ("acknowledge", "unacknowledge")
    };
    harness::run_write_ok(&["cases", first, &id, "-o", "json"]);
    let mid = case_of(&harness::run_ok_json(&["cases", "get", &id, "-o", "json"]));
    assert_ne!(
        mid["acknowledgeTime"].is_null(),
        before["acknowledgeTime"].is_null(),
        "{first} should flip the acknowledgement: {mid}"
    );
    harness::run_write_ok(&["cases", second, &id, "-o", "json"]);

    // set-priority -> clear-priority (restore any original override)
    harness::run_write_ok(&[
        "cases",
        "set-priority",
        &id,
        "--priority",
        "P5",
        "-o",
        "json",
    ]);
    let mid = case_of(&harness::run_ok_json(&["cases", "get", &id, "-o", "json"]));
    assert_eq!(
        mid.pointer("/priorityDetails/override")
            .and_then(Value::as_str),
        Some("CASE_PRIORITY_P5"),
        "set-priority should set the override: {mid}"
    );
    match original_override.as_deref() {
        Some(p) => {
            harness::run_write_ok(&["cases", "set-priority", &id, "--priority", p, "-o", "json"]);
        }
        None => {
            harness::run_write_ok(&["cases", "clear-priority", &id, "-o", "json"]);
        }
    }

    // comment: visible in the event timeline
    let text = harness::unique_name("comment");
    harness::run_write_ok(&["cases", "comment", &id, "--text", &text, "-o", "json"]);
    let events = harness::run_ok_json(&["cases", "events", "list", &id, "-o", "json"]);
    assert!(
        events.to_string().contains(&text),
        "events should contain our comment {text}: {events}"
    );

    // update --title, then restore
    let new_title = format!("{original_title} {}", harness::unique_name("title"));
    harness::run_write_ok(&["cases", "update", &id, "--title", &new_title, "-o", "json"]);
    let mid = case_of(&harness::run_ok_json(&["cases", "get", &id, "-o", "json"]));
    assert_eq!(mid["title"], new_title, "update should change the title");
    harness::run_write_ok(&[
        "cases",
        "update",
        &id,
        "--title",
        &original_title,
        "-o",
        "json",
    ]);

    // assign -> unassign (restore the original assignee if there was one)
    if let Some(user) = discover_assignee() {
        harness::run_write_ok(&["cases", "assign", &id, "--user", &user, "-o", "json"]);
        let mid = case_of(&harness::run_ok_json(&["cases", "get", &id, "-o", "json"]));
        assert!(
            !mid.pointer("/assignee/userId")
                .unwrap_or(&Value::Null)
                .is_null(),
            "assign should set an assignee: {mid}"
        );
        harness::run_write_ok(&["cases", "unassign", &id, "-o", "json"]);
        if let Some(orig) = original_assignee {
            harness::run_write_ok(&["cases", "assign", &id, "--user", &orig, "-o", "json"]);
        }
    }

    let after = case_of(&harness::run_ok_json(&["cases", "get", &id, "-o", "json"]));
    assert_eq!(after["title"], before["title"], "title restored");
    assert_eq!(
        after["acknowledgeTime"].is_null(),
        before["acknowledgeTime"].is_null(),
        "acknowledgement restored"
    );
    assert_eq!(
        after.pointer("/priorityDetails/override"),
        before.pointer("/priorityDetails/override"),
        "priority override restored"
    );
    eprintln!(
        "[e2e] not covered: cases resolve/close (success path) — irreversible on a real case; \
         cases update --resolution-reason — only meaningful on a resolved case"
    );
}

fn case_of(v: &Value) -> Value {
    v.get("case").cloned().unwrap_or_else(|| v.clone())
}

/// A known case ID for e2e checks, supplied via `CX_TEST_CASE_ID`.
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
            harness::require_creds("cases_events_discover")?;
            let id = discover_case_id()?;
            let stdout = harness::run_ok(&["cases", "events", "list", &id, "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter(|item| !harness::is_e2e_item(item))
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}

/// A teammate e-mail to assign to: `CX_TEST_CASE_ASSIGNEE` wins, else the
/// first user returned by `iam users search`. The API key used in CI is a
/// service key without a user of its own, so "the caller" is not an option.
fn discover_assignee() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if let Ok(user) = std::env::var("CX_TEST_CASE_ASSIGNEE") {
                if !user.is_empty() {
                    return Some(user);
                }
            }
            let out = harness::run_raw(&["iam", "users", "search", "-o", "json"]);
            if !out.status.success() {
                return None;
            }
            let v = harness::parse_json(&out.stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|u| u.get("email").and_then(Value::as_str))
                .next()
                .map(String::from)
        })
        .clone()
}
