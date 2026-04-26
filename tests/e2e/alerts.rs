use crate::harness;

#[test]
#[ignore]
fn alerts_list() {
    if harness::require_creds("alerts_list").is_none() {
        return;
    }
    harness::run_ok_json(&["alerts", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn alerts_get() {
    if harness::require_creds("alerts_get").is_none() {
        return;
    }
    let Some(id) = harness::discover_alert_id() else {
        eprintln!("[e2e] skipping alerts_get: no alerts available in staging");
        return;
    };
    harness::run_ok_json(&["alerts", "get", &id, "-o", "json"]);
}

// Mutating alert commands (`create`, `enable`, `disable`) are deliberately
// not covered yet. `create` has no companion delete; `enable`/`disable`
// would change shared staging state. Revisit when we're comfortable
// mutating staging from CI.
