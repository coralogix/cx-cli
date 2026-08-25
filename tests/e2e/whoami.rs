use crate::harness;

/// `cx whoami` against the real test team: a valid key must authenticate and
/// return an identity object carrying the region it resolved.
#[test]
#[ignore]
fn whoami_authenticates() {
    if harness::require_creds("whoami_authenticates").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["whoami", "-o", "json"]);
    // Single-profile whoami emits a bare object (render_json_auto), not an array.
    assert!(
        v.get("region").and_then(|r| r.as_str()).is_some(),
        "whoami json should carry a region string, got: {v}"
    );
}
