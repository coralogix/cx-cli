use crate::harness;

#[test]
#[ignore]
fn datasets_list() {
    if harness::require_creds("datasets_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["datasets", "list", "-o", "json"]);
    // The response is always an array of dataset rows; the test team always
    // has at least the `default/logs` system dataset.
    harness::assert_nonempty_array_of_objects_with_keys(
        &v,
        &[
            "teamId",
            "dataspace",
            "dataset",
            "version",
            "pillar",
            "entityType",
        ],
    );
}

#[test]
#[ignore]
fn datasets_list_with_filter() {
    if harness::require_creds("datasets_list_with_filter").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "datasets",
        "list",
        "--filter",
        "<v1> $d.pillar == 'logs'",
        "-o",
        "json",
    ]);
    let arr = harness::assert_array(&v);
    for item in arr {
        assert_eq!(
            item["pillar"].as_str(),
            Some("logs"),
            "filtered list must only contain logs-pillar datasets"
        );
    }
}
