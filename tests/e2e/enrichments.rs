use crate::harness;

#[test]
#[ignore]
fn enrichments_list() {
    if harness::require_creds("enrichments_list").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["enrichments", "list", "-o", "json"]);
}

#[test]
#[ignore]
fn enrichments_limit() {
    if harness::require_creds("enrichments_limit").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["enrichments", "limit", "-o", "json"]);
}

#[test]
#[ignore]
fn enrichments_settings() {
    if harness::require_creds("enrichments_settings").is_none() {
        return;
    }
    harness::run_ok_nonempty(&["enrichments", "settings", "-o", "json"]);
}
