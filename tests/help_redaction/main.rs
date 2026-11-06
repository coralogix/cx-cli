//! Integration tests ensuring `--help` never prints secret env-var values.
//!
//! clap shows `[env: NAME=<value>]` for env-backed args unless
//! `hide_env_values` is set. The global `--api-key` is backed by `CX_API_KEY`,
//! so without that flag every help screen would leak the key — and agents
//! routinely run `--help`, writing it into their session history.

use assert_cmd::Command;

const FAKE_KEY: &str = "cxup_HELP_REDACTION_SHOULD_NOT_APPEAR";

fn help_output(args: &[&str]) -> String {
    let output = Command::cargo_bin("cx")
        .expect("cx binary should build")
        .args(args)
        .env("CX_API_KEY", FAKE_KEY)
        .output()
        .expect("run cx");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn help_does_not_print_api_key_from_env() {
    for args in [
        &["--help"][..],
        &["-h"],
        &["dashboards", "--help"],
        &["dashboards", "-h"],
        &["logs", "--help"],
        &["profiles", "add", "--help"],
    ] {
        let text = help_output(args);
        assert!(
            text.contains("CX_API_KEY"),
            "`cx {}` help should still mention CX_API_KEY",
            args.join(" ")
        );
        assert!(
            !text.contains(FAKE_KEY),
            "`cx {}` help leaked the CX_API_KEY value",
            args.join(" ")
        );
    }
}
