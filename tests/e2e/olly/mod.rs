use crate::harness;

#[test]
#[ignore]
fn olly_ask_basic() {
    if harness::require_creds("olly_ask_basic").is_none() {
        return;
    }
    // Send a simple message and verify we get a response
    // Note: This creates a new chat each time - keep messages simple to avoid long responses
    let v = harness::run_ok_json(&["olly", "ask", "Say hello in one word", "-o", "json"]);

    // Response should be an array with one object containing chat_id, interaction_id, status
    let arr = v.as_array().expect("expected array response");
    assert!(!arr.is_empty(), "expected at least one response object");

    let obj = &arr[0];
    assert!(obj.get("chat_id").is_some(), "expected chat_id in response");
    assert!(
        obj.get("interaction_id").is_some(),
        "expected interaction_id in response"
    );
    assert!(obj.get("status").is_some(), "expected status in response");
}

#[test]
#[ignore]
fn olly_ask_text_output() {
    if harness::require_creds("olly_ask_text_output").is_none() {
        return;
    }
    // Verify text output mode works (default)
    let stdout_bytes = harness::run_ok(&["olly", "ask", "Reply with OK"]);
    let stdout = String::from_utf8_lossy(&stdout_bytes);

    // Text output should contain "Chat ID:" line
    assert!(
        stdout.contains("Chat ID:"),
        "expected 'Chat ID:' in text output"
    );
}
