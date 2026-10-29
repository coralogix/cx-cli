use crate::harness;

#[test]
#[ignore]
fn dataprime_query_logs() {
    if harness::require_creds("dataprime_query_logs").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "dataprime",
        "query",
        "--source",
        "logs",
        "limit 1",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        harness::SMALL_LIMIT,
        "-o",
        "json",
    ]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn dataprime_strict_validation_and_extended_information() {
    if harness::require_creds("dataprime_strict_validation_and_extended_information").is_none() {
        return;
    }
    let query = "source logs | create missing from $d.cx_cli_missing_field_8b973d6a | limit 1";
    let strict = harness::cx()
        .args([
            "dataprime",
            "query",
            query,
            "--start",
            harness::SHORT_WINDOW_START,
            "--limit",
            "1",
            "--strict-fields-validation",
            "-o",
            "json",
        ])
        .output()
        .expect("cx must run");
    assert!(
        !strict.status.success(),
        "unknown keypaths must fail with --strict-fields-validation"
    );
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(stderr.contains("keypath does not exist"), "{stderr}");
    assert!(
        stderr.contains("--strict-fields-validation=false"),
        "strict keypath errors must name the flag that turns validation off: {stderr}"
    );
    assert!(
        stderr.contains("strict_fields_validation"),
        "strict keypath errors must name strict_fields_validation in ~/.cx/config.toml: {stderr}"
    );
    let permissive = harness::run_ok_json(&[
        "dataprime",
        "query",
        query,
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        "1",
        "--extended-information",
        "-o",
        "json",
    ]);
    assert!(permissive["results"].is_array());
    assert!(permissive["queryId"].is_string());
    assert_eq!(permissive["statistics"]["status"], "COMPLETED");
    assert!(permissive["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.to_string().contains("keypath does not exist")));
}
