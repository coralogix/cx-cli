use crate::harness;

#[test]
#[ignore]
fn logs_basic() {
    if harness::require_creds("logs_basic").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "logs",
        "limit 1",
        "--start",
        harness::SHORT_WINDOW_START,
        "--limit",
        harness::SMALL_LIMIT,
        "-o",
        "json",
    ]);
    // Row shape varies by query (aggregates, projections); just verify the
    // top-level container is the expected array of rows.
    harness::assert_array(&v);
}
