//! E2E sanity checks for `cx ai-center` against a real Coralogix test team.
//!
//! Read-only only: these verify the commands run end-to-end (exit 0, valid JSON).
//! Mutating operations (create/update/delete, policy link/unlink, set pricing) are
//! deliberately NOT exercised — they touch shared test-team state.

use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn ai_center_applications_list() {
    if harness::require_creds("ai_center_applications_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["ai-center", "applications", "list", "-o", "json"]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn ai_center_applications_get() {
    if harness::require_creds("ai_center_applications_get").is_none() {
        return;
    }
    let Some(id) = discover_application_id() else {
        eprintln!("[e2e] skipping ai_center_applications_get: no AI applications on test team");
        return;
    };
    harness::run_ok_json(&["ai-center", "applications", "get", &id, "-o", "json"]);
}

#[test]
#[ignore]
fn ai_center_evaluations_list() {
    if harness::require_creds("ai_center_evaluations_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["ai-center", "evaluations", "list", "-o", "json"]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn ai_center_count() {
    if harness::require_creds("ai_center_count").is_none() {
        return;
    }
    // Count returns an object mapping eval type -> app count; just assert valid JSON + exit 0.
    harness::run_ok_json(&["ai-center", "count", "-o", "json"]);
}

#[test]
#[ignore]
fn ai_center_custom_evaluations_list() {
    if harness::require_creds("ai_center_custom_evaluations_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["ai-center", "custom-evaluations", "list", "-o", "json"]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn ai_center_model_pricing_get() {
    if harness::require_creds("ai_center_model_pricing_get").is_none() {
        return;
    }
    harness::run_ok_json(&["ai-center", "model-pricing", "get", "-o", "json"]);
}

/// Discover an application id from `applications list -o json`. Cached so multiple
/// tests don't each pay for the list call; returns `None` when the team has no AI apps.
fn discover_application_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["ai-center", "applications", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter_map(|item| item.get("id").and_then(|x| x.as_str()))
                .next()
                .map(String::from)
        })
        .clone()
}

// Mutating commands (evaluations create/update/delete, custom-evaluations
// create/update, add-policy/remove-policy, model-pricing set) are intentionally
// left uncovered in e2e: they mutate shared test-team configuration and there is no
// paired-undo plan. Exercise them manually with --yes against a disposable team.
