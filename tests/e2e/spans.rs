use crate::harness;

#[test]
#[ignore]
fn spans_basic() {
    if harness::require_creds("spans_basic").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "spans",
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
