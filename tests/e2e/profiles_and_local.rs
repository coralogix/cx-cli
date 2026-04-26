//! Sanity tests for commands that don't hit the Coralogix API.
//!
//! These still gate on `require_creds` so the entire e2e suite is uniformly
//! opt-in — running e2e without staging access doesn't make sense even for
//! local-only commands.

use crate::harness;

#[test]
#[ignore]
fn profiles_list() {
    if harness::require_creds("profiles_list").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["profiles", "list"]);
}

#[test]
#[ignore]
fn cleanup() {
    if harness::require_creds("cleanup").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["cleanup"]);
}

#[test]
#[ignore]
fn dataprime_list() {
    if harness::require_creds("dataprime_list").is_none() {
        return;
    }
    harness::run_ok_json(&["dataprime", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn dataprime_show() {
    if harness::require_creds("dataprime_show").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["dataprime", "show", "source"]);
}
