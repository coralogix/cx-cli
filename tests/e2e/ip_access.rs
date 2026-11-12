use crate::harness;

#[test]
#[ignore]
fn ip_access_get() {
    if harness::require_creds("ip_access_get").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["iam", "ip-access", "get", "-o", "json"]);
    harness::assert_get_response(&v, &["id", "ipAccess"]);
    harness::run_ok_nonempty(&["iam", "ip-access", "get"]);
    harness::run_ok_nonempty(&["iam", "ip-access", "get", "-o", "toon"]);
}

/// `iam ip-access create|update|delete` change the team-wide login IP
/// allowlist. The CI runners' egress IPs are unknown, so any rule the test
/// could write risks locking every user and the suite itself out of the
/// team, and `delete` would drop a policy the team may rely on. Read-only
/// coverage only.
#[test]
#[ignore]
fn ip_access_write_commands_not_covered() {
    if harness::require_creds("ip_access_write_commands_not_covered").is_none() {
        return;
    }
    eprintln!("[e2e] not covered: iam ip-access create — could lock CI and users out of the team");
    eprintln!("[e2e] not covered: iam ip-access update — could lock CI and users out of the team");
    eprintln!("[e2e] not covered: iam ip-access delete — removes the team-wide access policy");
}
