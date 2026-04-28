use crate::harness;

#[test]
#[ignore]
fn saml_get() {
    if harness::require_creds("saml_get").is_none() {
        return;
    }
    // SAML configuration endpoints require org-admin permissions that the
    // test API key may not have. Skip gracefully on auth errors.
    let _v = harness::run_tolerant_json(&["iam", "saml", "get", "-o", "json"], "saml_get");
}

#[test]
#[ignore]
fn saml_sp_params() {
    if harness::require_creds("saml_sp_params").is_none() {
        return;
    }
    let _v = harness::run_tolerant_json(
        &["iam", "saml", "sp-params", "-o", "json"],
        "saml_sp_params",
    );
}
