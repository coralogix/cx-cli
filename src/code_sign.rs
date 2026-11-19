//! Whether this macOS process is signed with an Apple Developer ID certificate.
//!
//! A Keychain item created through the Security framework is ACL'd to the
//! calling binary. An ad-hoc signature is stored against that build's hash, so
//! it does not match the next Homebrew upgrade. An Apple-issued Developer ID
//! signature is not ad-hoc. Certificate renewal keeps that property, so the
//! check does not name a team id.
//!
//! Unsigned and ad-hoc binaries store Keychain items through `/usr/bin/security`
//! instead. Any failure to read the signature is treated as unsigned, so those
//! builds take that path.

use std::str::FromStr;

use security_framework::os::macos::code_signing::{Flags, SecCode, SecRequirement};

/// Passes for any Apple-issued Developer ID signature, and fails for an
/// ad-hoc or unsigned binary.
const APPLE_SIGNED_REQUIREMENT: &str = "anchor apple generic";

/// `true` when this process satisfies [`APPLE_SIGNED_REQUIREMENT`].
///
/// Any failure to read or validate the signature is treated as unsigned, so
/// credential storage uses `/usr/bin/security` rather than the direct Keychain
/// ACL.
pub fn has_apple_developer_id() -> bool {
    let Ok(code) = SecCode::for_self(Flags::NONE) else {
        return false;
    };
    let Ok(requirement) = SecRequirement::from_str(APPLE_SIGNED_REQUIREMENT) else {
        return false;
    };
    code.check_validity(Flags::NONE, &requirement).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_is_any_apple_certificate() {
        assert_eq!(APPLE_SIGNED_REQUIREMENT, "anchor apple generic");
    }

    #[test]
    fn cargo_test_binary_is_not_apple_signed() {
        // `cargo test` produces an ad-hoc binary. A Developer ID signature
        // would make this assertion wrong, which is the signal to revisit it.
        assert!(!has_apple_developer_id());
    }
}
