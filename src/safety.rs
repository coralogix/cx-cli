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

pub fn confirm_destructive(action: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
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
}
