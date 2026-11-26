use std::io::IsTerminal;

use anyhow::{bail, Result};
use inquire::validator::MinLengthValidator;
use inquire::{Confirm, Text};

const WRITE_VERBS: &[&str] = &[
    "acknowledge",
    "activate",
    "add",
    "archive",
    "assign",
    "batch",
    "bulk-delete",
    "clear-priority",
    "close",
    "create",
    "delete",
    "deploy",
    "disable",
    "enable",
    "overwrite",
    "remove",
    "reorder",
    "replace",
    "resolve",
    "run",
    "set",
    "set-active",
    "set-default",
    "set-idp",
    "set-priority",
    "set-schedule",
    "set-status",
    "settings-update",
    "share",
    "stop",
    "unacknowledge",
    "unarchive",
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

/// Every subcommand name on the path to the leaf, for example
/// `["alerts", "delete"]`.
pub fn get_subcommand_path(matches: &clap::ArgMatches) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, sub)) = current.subcommand() {
        path.push(name.to_string());
        current = sub;
    }
    path
}

/// The stderr line printed before a write command runs against more than one
/// profile (repeated `-p`, or `--label`), so the blast radius is visible in a
/// terminal, in logs and in an agent's session. `None` for a single profile
/// or for a read command.
pub fn multi_profile_write_notice(command_path: &[String], profiles: &[String]) -> Option<String> {
    let leaf = command_path.last()?;
    if profiles.len() < 2 || !is_write_verb(leaf) {
        return None;
    }
    Some(format!(
        "Running '{}' against {} profiles: {}",
        command_path.join(" "),
        profiles.len(),
        profiles.join(", ")
    ))
}

pub fn env_is_truthy(var: &str) -> bool {
    std::env::var(var)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

pub fn enforce_read_only(verb: &str) -> Result<()> {
    if is_write_verb(verb) {
        bail!(
            "Write operation '{verb}' is blocked in read-only mode \
             (--read-only flag, CX_READ_ONLY env var, or read_only = true in ~/.cx/config.toml)."
        );
    }
    Ok(())
}

pub(crate) const AGENT_ENV_VARS: &[&str] = &[
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
    AGENT_ENV_VARS.iter().any(|var| std::env::var(var).is_ok())
}

/// Interactively prompt the user for a required non-empty text value.
///
/// Returns `Ok(Some(value))` when the user supplied a string in an interactive
/// terminal, and `Ok(None)` when prompting is skipped because the user opted
/// out (`yes == true`), is non-interactive (no TTY), or is running under an
/// agent. Callers should treat `None` as "fall back to the value already
/// provided by the caller / leave the field unset."
pub fn prompt_optional_text(
    label: &str,
    help: Option<&str>,
    yes: bool,
    agent_mode: bool,
) -> Result<Option<String>> {
    if yes || agent_mode || !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut prompt = Text::new(label).with_validator(MinLengthValidator::new(1));
    if let Some(h) = help {
        prompt = prompt.with_help_message(h);
    }
    Ok(Some(prompt.prompt()?))
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

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn multi_profile_write_notice_names_command_and_every_profile() {
        let line =
            multi_profile_write_notice(&names(&["alerts", "delete"]), &names(&["a", "b", "c"]));
        assert_eq!(
            line.as_deref(),
            Some("Running 'alerts delete' against 3 profiles: a, b, c")
        );
    }

    #[test]
    fn multi_profile_write_notice_is_silent_for_one_profile_or_a_read() {
        assert!(
            multi_profile_write_notice(&names(&["alerts", "delete"]), &names(&["a"])).is_none()
        );
        assert!(
            multi_profile_write_notice(&names(&["alerts", "list"]), &names(&["a", "b"])).is_none()
        );
        assert!(multi_profile_write_notice(&[], &names(&["a", "b"])).is_none());
    }

    #[test]
    fn test_write_verbs_sorted() {
        let mut sorted = WRITE_VERBS.to_vec();
        sorted.sort();
        assert_eq!(
            WRITE_VERBS,
            &sorted[..],
            "WRITE_VERBS must be sorted for binary_search"
        );
    }

    #[test]
    fn test_write_verbs_detected() {
        let verbs = [
            "create",
            "update",
            "delete",
            "set",
            "enable",
            "disable",
            "deploy",
            "undeploy",
            "reorder",
            "overwrite",
            "remove",
            "add",
            "acknowledge",
            "resolve",
            "close",
            "assign",
            "unassign",
            "bulk-delete",
            "set-status",
            "set-idp",
            "set-active",
            "set-default",
            "set-priority",
            "clear-priority",
            "unacknowledge",
            "batch",
            "activate",
            "settings-update",
            "archive",
            "unarchive",
            "run",
            "set-schedule",
            "share",
        ];
        for verb in verbs {
            assert!(
                is_write_verb(verb),
                "{verb} should be detected as a write verb"
            );
        }
    }

    #[test]
    fn test_read_verbs_not_detected() {
        let verbs = [
            "list",
            "get",
            "search",
            "catalog",
            "show",
            "system",
            "sp-params",
            "validate",
            "settings",
            "deployed",
            "query",
            "test",
            "send-data-keys",
        ];
        for verb in verbs {
            assert!(
                !is_write_verb(verb),
                "{verb} should NOT be detected as a write verb"
            );
        }
    }

    #[test]
    fn test_unknown_verbs_not_detected() {
        for verb in ["foo", "bar", "", "unknown-verb"] {
            assert!(
                !is_write_verb(verb),
                "{verb} should NOT be detected as a write verb"
            );
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
    fn test_prompt_optional_text_returns_none_with_yes() {
        let r = prompt_optional_text("label", None, true, false).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn test_prompt_optional_text_returns_none_in_agent_mode() {
        let r = prompt_optional_text("label", None, false, true).unwrap();
        assert!(r.is_none());
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
