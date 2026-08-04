use std::io::IsTerminal;

use anyhow::{bail, Result};
use inquire::validator::MinLengthValidator;
use inquire::{Confirm, Text};

const WRITE_VERBS: &[&str] = &[
    "acknowledge",
    "activate",
    "add",
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
    "set",
    "set-active",
    "set-default",
    "set-idp",
    "set-priority",
    "set-status",
    "settings-update",
    "unacknowledge",
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
    std::env::var(var)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

/// Agent identity inferred from known environment variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentInvoker {
    Aider,
    AmazonQ,
    AwsQDeveloper,
    ClaudeCode,
    Cline,
    Codex,
    Copilot,
    Cursor,
    CxAgentMode,
    GeminiCodeAssist,
    GithubCopilot,
    OpenAiCodex,
    SrcCody,
    Windsurf,
    Human,
    Unknown,
}

impl EnvironmentInvoker {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Aider => "aider",
            Self::AmazonQ => "amazon_q",
            Self::AwsQDeveloper => "aws_q_developer",
            Self::ClaudeCode => "claude_code",
            Self::Cline => "cline",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::CxAgentMode => "cx_agent_mode",
            Self::GeminiCodeAssist => "gemini_code_assist",
            Self::GithubCopilot => "github_copilot",
            Self::OpenAiCodex => "openai_codex",
            Self::SrcCody => "src_cody",
            Self::Windsurf => "windsurf",
            Self::Human => "human",
            Self::Unknown => "unknown",
        }
    }
}

const AGENT_ENV_VARS: &[(&str, EnvironmentInvoker)] = &[
    ("AIDER", EnvironmentInvoker::Aider),
    ("AMAZON_Q", EnvironmentInvoker::AmazonQ),
    ("AWS_Q_DEVELOPER", EnvironmentInvoker::AwsQDeveloper),
    ("CLAUDECODE", EnvironmentInvoker::ClaudeCode),
    ("CLAUDE_CODE", EnvironmentInvoker::ClaudeCode),
    ("CLINE", EnvironmentInvoker::Cline),
    ("CODEX", EnvironmentInvoker::Codex),
    ("COPILOT_AGENT", EnvironmentInvoker::Copilot),
    ("CURSOR_AGENT", EnvironmentInvoker::Cursor),
    ("CX_AGENT_MODE", EnvironmentInvoker::CxAgentMode),
    ("GEMINI_CODE_ASSIST", EnvironmentInvoker::GeminiCodeAssist),
    ("GITHUB_COPILOT", EnvironmentInvoker::GithubCopilot),
    ("OPENAI_CODEX", EnvironmentInvoker::OpenAiCodex),
    ("SRC_CODY", EnvironmentInvoker::SrcCody),
    ("WINDSURF_AGENT", EnvironmentInvoker::Windsurf),
];

/// Returns the agent name inferred from the same environment variables used in
/// master, or `human` / `unknown` when no known agent environment is present.
pub fn environment_invoker_name() -> &'static str {
    AGENT_ENV_VARS
        .iter()
        .find(|(variable, _)| std::env::var(variable).is_ok())
        .map(|(_, invoker)| invoker.as_str())
        .unwrap_or_else(|| {
            if std::io::stdin().is_terminal() {
                EnvironmentInvoker::Human.as_str()
            } else {
                EnvironmentInvoker::Unknown.as_str()
            }
        })
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

pub fn is_agent_mode() -> bool {
    std::env::var("CX_AGENT_NAME")
        .ok()
        .is_some_and(|name| !name.trim().is_empty())
}

/// Returns the configured agent name, `human` for a terminal, or `unknown`.
pub fn invoker_name() -> String {
    std::env::var("CX_AGENT_NAME")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            if std::io::stdin().is_terminal() {
                "human".to_string()
            } else {
                "unknown".to_string()
            }
        })
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
}
