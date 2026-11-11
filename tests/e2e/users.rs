use std::sync::OnceLock;

use crate::harness;
use serde_json::Value;

const USER_KEYS: &[&str] = &["user_id", "user_account_id", "name", "email", "status"];

#[test]
#[ignore]
fn users_search() {
    if harness::require_creds("users_search").is_none() {
        return;
    }
    // The test team has seed members.
    let v = harness::run_ok_json(&["iam", "users", "search", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, USER_KEYS);
}

#[test]
#[ignore]
fn users_search_with_filters() {
    if harness::require_creds("users_search_with_filters").is_none() {
        return;
    }
    // --status + --page-size
    let page = harness::run_ok_json(&[
        "iam",
        "users",
        "search",
        "--status",
        "USER_STATUS_ACTIVE",
        "--page-size",
        "2",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&page, USER_KEYS);
    assert!(
        page.as_array().is_some_and(|a| a.len() <= 2),
        "--page-size 2 should cap the page: {page}"
    );
    for u in page.as_array().unwrap() {
        assert_eq!(u["status"], "USER_STATUS_ACTIVE");
    }

    // --page-token: the second page of size 2 is a (possibly empty) user list
    let next = harness::run_ok_json(&[
        "iam",
        "users",
        "search",
        "--page-size",
        "2",
        "--page-token",
        "1",
        "-o",
        "json",
    ]);
    harness::assert_array_of_objects_with_keys(&next, USER_KEYS);

    // --query: search by the e-mail of a known member; shape only (the
    // server-side matching rules are not under test here)
    if let Some(email) = discover_user().and_then(|u| u["email"].as_str().map(str::to_string)) {
        let hits =
            harness::run_ok_json(&["iam", "users", "search", "--query", &email, "-o", "json"]);
        harness::assert_array_of_objects_with_keys(&hits, USER_KEYS);
    }
}

#[test]
#[ignore]
fn users_get() {
    if harness::require_creds("users_get").is_none() {
        return;
    }
    let Some(user) = discover_user() else {
        eprintln!("[e2e] skipping users_get: no users on test team");
        return;
    };
    // `users get` takes the numeric user account id.
    let account_id = user["user_account_id"]
        .as_i64()
        .unwrap_or_else(|| panic!("user has no numeric user_account_id: {user}"))
        .to_string();
    let v = harness::run_ok_json(&["iam", "users", "get", &account_id, "-o", "json"]);
    harness::assert_get_response(&v, &["userId", "userAccountId", "username", "status"]);
    assert_eq!(
        v["user"]["userAccountId"].as_i64().map(|n| n.to_string()),
        Some(account_id.clone())
    );
    assert_eq!(v["user"]["userId"], user["user_id"]);

    // text + toon renderers
    harness::run_ok_nonempty(&["iam", "users", "get", &account_id]);
    harness::run_ok_nonempty(&["iam", "users", "get", &account_id, "-o", "toon"]);
}

#[test]
#[ignore]
fn users_get_nonexistent_fails() {
    if harness::require_creds("users_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&["iam", "users", "get", "1", "-o", "json"]);
}

/// `iam users create`, `iam users update` and `iam users set-status` are not
/// exercised: creating a user sends a real invitation e-mail, and update /
/// set-status act on real team members (set-status INACTIVE locks a person
/// out). There is no throw-away user on the test team to target safely.
#[test]
#[ignore]
fn users_write_commands_not_covered() {
    if harness::require_creds("users_write_commands_not_covered").is_none() {
        return;
    }
    eprintln!("[e2e] not covered: iam users create — invites a real e-mail address");
    eprintln!("[e2e] not covered: iam users update — modifies a real team member");
    eprintln!("[e2e] not covered: iam users set-status — can lock a real team member out");
}

fn discover_user() -> Option<Value> {
    static CACHE: OnceLock<Option<Value>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("users_discover")?;
            let stdout = harness::run_ok(&["iam", "users", "search", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find(|u| u["user_account_id"].is_number())
                .cloned()
        })
        .clone()
}
