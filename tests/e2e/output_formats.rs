//! Smoke tests that exercise the three output formats on one representative
//! read-only command. We don't sweep every command across every format — that
//! would 3x the suite — but we want a tripwire that catches a broken
//! rendering path in any of the three modes.

use crate::harness;

#[test]
#[ignore]
fn alerts_list_text() {
    if harness::require_creds("alerts_list_text").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["alerts", "list", "-o", "text"]);
}

#[test]
#[ignore]
fn alerts_list_json() {
    if harness::require_creds("alerts_list_json").is_none() {
        return;
    }
    harness::run_ok_json(&["alerts", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn alerts_list_agents() {
    if harness::require_creds("alerts_list_agents").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["alerts", "list", "-o", "agents"]);
}
