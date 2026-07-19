use crate::harness;

#[test]
#[ignore]
fn datasets_list_json() {
    if harness::require_creds("datasets_list_json").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["datasets", "list", "-o", "json"]);
    // Multi-profile may wrap as an array of listings; single-profile is one object
    // or a single-element array depending on render_json_auto.
    let obj = if let Some(arr) = v.as_array() {
        arr.first().expect("expected at least one listing")
    } else {
        &v
    };
    assert!(
        obj.get("alwaysAvailableSources").is_some(),
        "missing alwaysAvailableSources: {obj}"
    );
    assert!(obj.get("datasets").is_some(), "missing datasets: {obj}");
    assert!(
        obj.get("datasetCounts").is_some(),
        "missing datasetCounts: {obj}"
    );
}
