use crate::harness;

#[test]
#[ignore]
fn saml_get() {
    if harness::require_creds("saml_get").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["saml", "get", "-o", "json"]);
}

#[test]
#[ignore]
fn saml_sp_params() {
    if harness::require_creds("saml_sp_params").is_none() {
        return;
    }
    let _v = harness::run_ok_json(&["saml", "sp-params", "-o", "json"]);
}
