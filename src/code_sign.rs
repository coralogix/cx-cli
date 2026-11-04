//! Whether this macOS process carries the stable Developer ID signature.
//!
//! A Keychain "Always Allow" entry is stored against the binary's signing
//! requirement. The release binary keeps one requirement across upgrades.
//! An ad-hoc signature (a local build, or a bottle built without that
//! identity) gets a new hash every build, so the entry does not match again.

use std::str::FromStr;

use security_framework::os::macos::code_signing::{Flags, SecCode, SecRequirement};

/// Designated requirement of the signed release binary. The team and
/// identifier are what a Keychain ACL matches across versions.
const STABLE_SIGNING_REQUIREMENT: &str = concat!(
    r#"identifier "com.coralogix.cx" "#,
    r#"and anchor apple generic "#,
    r#"and certificate leaf[subject.OU] = "5566BG6FW3""#,
);

/// `true` when this process satisfies [`STABLE_SIGNING_REQUIREMENT`].
///
/// Any failure to read or validate the signature is treated as unsigned, so
/// setup does not offer the Keychain.
pub fn has_stable_developer_id() -> bool {
    let Ok(code) = SecCode::for_self(Flags::NONE) else {
        return false;
    };
    let Ok(requirement) = SecRequirement::from_str(STABLE_SIGNING_REQUIREMENT) else {
        return false;
    };
    code.check_validity(Flags::NONE, &requirement).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_names_the_release_identity() {
        assert!(STABLE_SIGNING_REQUIREMENT.contains("com.coralogix.cx"));
        assert!(STABLE_SIGNING_REQUIREMENT.contains("5566BG6FW3"));
    }

    #[test]
    fn cargo_test_binary_is_not_the_signed_release() {
        // `cargo test` produces an ad-hoc binary. A Developer ID signature
        // would make this assertion wrong, which is the signal to revisit it.
        assert!(!has_stable_developer_id());
    }
}
