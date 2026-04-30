use std::io::IsTerminal;

use anyhow::{bail, Result};
use inquire::Confirm;

const WRITE_VERBS: &[&str] = &[
    "acknowledge",
    "activate",
    "add",
    "assign",
    "batch",
    "bulk-delete",
    "close",
    "create",
    "delete",
    "deploy",
    "disable",
    "enable",
    "overwrite",
    "remove",
    "reorder",
    "resolve",
    "set",
    "set-active",
    "set-default",
    "set-idp",
    "set-status",
    "settings-update",
    "unassign",
    "undeploy",
    "update",
];

pub fn is_write_verb(name: &str) -> bool {
    WRITE_VERBS.binary_search(&name).is_ok()
}

pub fn get_leaf_subcommand_name(matches: &clap::ArgMatches) -> Option<String> {
    match matches.subcommand() {
        Some((name, sub)) => get_leaf_subcommand_name(sub).or(Some(name.to_string())),
        None => None,
    }
}

pub fn get_top_level_subcommand_name(matches: &clap::ArgMatches) -> Option<String> {
    matches.subcommand().map(|(name, _)| name.to_string())
}

pub fn env_is_truthy(var: &str) -> bool {
    matches!(
        std::env::var(var).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

pub fn enforce_read_only(verb: &str) -> Result<()> {
    if is_write_verb(verb) {
        bail!(
            "Write operation '{verb}' is blocked in read-only mode \
             (--read-only flag or CX_READ_ONLY env var)."
        );
    }
    Ok(())
}

const AGENT_ENV_VARS: &[&str] = &[
    "AIDER",
    "AMAZON_Q",
    "AWS_Q_DEVELOPER",
    "CLAUDECODE",
    "CLAUDE_CODE",
    "CLINE",
    "CODEX",
    "COPILOT_AGENT",
    "CURSOR_AGENT",
    "CX_AGENT_MODE",
    "GEMINI_CODE_ASSIST",
    "GITHUB_COPILOT",
    "OPENAI_CODEX",
    "SRC_CODY",
    "WINDSURF_AGENT",
];

pub fn is_agent_mode() -> bool {
    AGENT_ENV_VARS
        .iter()
        .any(|var| std::env::var(var).is_ok())
}

pub fn confirm_destructive(action: &str, yes: bool, agent_mode: bool) -> Result<()> {
    if yes {
        eprintln!("[auto-approved via --yes] {action}");
        return Ok(());
    }
    if agent_mode {
        bail!(
            "This operation requires user confirmation: {action}\n\
             You are running in agent mode. Ask the user to confirm this \
             operation, then re-run with --yes to proceed."
        );
    }
    if !std::io::stdin().is_terminal() {
        bail!(
            "This operation requires confirmation but stdin is not a terminal.\n\
             Pass --yes to skip the confirmation prompt."
        );
    }
    let confirmed = Confirm::new(action).with_default(false).prompt()?;
    if !confirmed {
        bail!("Cancelled.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_verbs_sorted() {
        let mut sorted = WRITE_VERBS.to_vec();
        sorted.sort();
        assert_eq!(WRITE_VERBS, &sorted[..], "WRITE_VERBS must be sorted for binary_search");
    }

    #[test]
    fn test_write_verbs_detected() {
        let verbs = [
            "create", "update", "delete", "set", "enable", "disable", "deploy", "undeploy",
            "reorder", "overwrite", "remove", "add", "acknowledge", "resolve", "close", "assign",
            "unassign", "bulk-delete", "set-status", "set-idp", "set-active", "set-default",
            "batch", "activate", "settings-update",
        ];
        for verb in verbs {
            assert!(is_write_verb(verb), "{verb} should be detected as a write verb");
        }
    }

    #[test]
    fn test_read_verbs_not_detected() {
        let verbs = [
            "list", "get", "search", "catalog", "show", "system", "sp-params", "validate",
            "settings", "deployed", "query", "test", "send-data-keys",
        ];
        for verb in verbs {
            assert!(!is_write_verb(verb), "{verb} should NOT be detected as a write verb");
        }
    }

    #[test]
    fn test_unknown_verbs_not_detected() {
        for verb in ["foo", "bar", "", "unknown-verb"] {
            assert!(!is_write_verb(verb), "{verb} should NOT be detected as a write verb");
        }
    }

    #[test]
    fn test_enforce_read_only_blocks_write_verb() {
        let err = enforce_read_only("delete").unwrap_err();
        assert!(
            err.to_string().contains("read-only mode"),
            "expected read-only error, got: {err}"
        );
    }

    #[test]
    fn test_enforce_read_only_allows_read_verb() {
        assert!(enforce_read_only("list").is_ok());
        assert!(enforce_read_only("get").is_ok());
        assert!(enforce_read_only("query").is_ok());
    }

    #[test]
    fn test_confirm_destructive_fails_in_agent_mode_without_yes() {
        let err = confirm_destructive("test action?", false, true).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("requires user confirmation"), "got: {msg}");
        assert!(msg.contains("--yes"), "should mention --yes, got: {msg}");
    }

    #[test]
    fn test_confirm_destructive_succeeds_with_yes_in_agent_mode() {
        assert!(confirm_destructive("test action?", true, true).is_ok());
    }

    #[test]
    fn test_confirm_destructive_succeeds_with_yes_no_agent() {
        assert!(confirm_destructive("test action?", true, false).is_ok());
    }

    #[test]
    fn test_agent_env_vars_sorted() {
        let mut sorted = AGENT_ENV_VARS.to_vec();
        sorted.sort();
        assert_eq!(
            AGENT_ENV_VARS,
            &sorted[..],
            "AGENT_ENV_VARS must be sorted alphabetically"
        );
    }
}
