use crate::harness;

#[test]
#[ignore]
fn dataprime_query_logs() {
    if harness::require_creds("dataprime_query_logs").is_none() {
        return;
    }
    harness::run_ok_json(&[
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
}
