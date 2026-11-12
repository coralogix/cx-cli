//! macOS Keychain access through Apple's `/usr/bin/security`.
//!
//! Homebrew-core builds are unsigned (Intel) or ad-hoc signed (Apple silicon).
//! A Keychain ACL created by the `cx` process itself is tied to that build's
//! code hash, so every `brew upgrade` asks for access again. `/usr/bin/security`
//! is Apple-signed and does not change across those upgrades, so items it
//! creates stay readable without a new prompt.
//!
//! The trade-off is that any process running as the user can read the secret
//! by invoking the same tool. Developer ID-signed builds do not use this
//! module; they keep the direct Security-framework path in `keyring_store`.
//!
//! `security add-generic-password -w` takes the secret as an argument, which
//! would show up in the process list. `security -i` reads the command from
//! stdin and parses it in-process, so the secret is not part of `argv`. That
//! parser accepts one line of at most [`INTERACTIVE_LINE_MAX`] bytes and has
//! no second channel for the password. A secret that does not fit, or that
//! contains a newline, is rejected here; callers fall back to the profile file.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Absolute path of the system tool. A relative `security` on `PATH` could be
/// something else; the Keychain ACL has to be the Apple-signed binary.
pub(crate) const SECURITY_PATH: &str = "/usr/bin/security";

/// `security -i` reads a command into a 4096-byte buffer (`MAX_LINE_LEN` in
/// Apple's Security tool), including the terminating NUL. The command text
/// and its trailing newline have to fit in the remaining bytes.
const INTERACTIVE_LINE_MAX: usize = 4095;

/// `errSecItemNotFound` (-25300), truncated to a process exit status.
const EXIT_ITEM_NOT_FOUND: i32 = 44;

/// `errSecInteractionNotAllowed` (-25308), truncated to a process exit status.
const EXIT_INTERACTION_NOT_ALLOWED: i32 = 36;

/// Which Keychain implementation a build should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    /// `keyring` crate / Security framework. ACL follows the caller's signature.
    NativeKeyring,
    /// `/usr/bin/security`. Used for unsigned and ad-hoc macOS binaries.
    SecurityCli,
}

/// `SecurityCli` only on macOS when this process is not Developer ID-signed.
pub(crate) fn select_backend(target_is_macos: bool, apple_developer_id: bool) -> Backend {
    if target_is_macos && !apple_developer_id {
        Backend::SecurityCli
    } else {
        Backend::NativeKeyring
    }
}

/// One `/usr/bin/security` invocation. The secret, when there is one, is in
/// `stdin` and not in `args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecurityCommand {
    pub program: &'static str,
    pub args: Vec<String>,
    pub stdin: String,
}

/// Captured result of a [`SecurityCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecurityOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs `security`. Tests supply a fake; macOS credential I/O uses
/// [`SystemSecurityRunner`].
pub(crate) trait SecurityRunner {
    fn run(&self, command: &SecurityCommand) -> Result<SecurityOutput>;
}

/// Spawns `/usr/bin/security` and feeds the interactive command on stdin.
pub(crate) struct SystemSecurityRunner;

impl SecurityRunner for SystemSecurityRunner {
    fn run(&self, command: &SecurityCommand) -> Result<SecurityOutput> {
        let mut child = Command::new(command.program)
            .args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to run {}", command.program))?;
        let mut stdin = child
            .stdin
            .take()
            .with_context(|| format!("{} did not accept stdin", command.program))?;
        stdin
            .write_all(command.stdin.as_bytes())
            .with_context(|| format!("failed to write to {} stdin", command.program))?;
        drop(stdin);
        let output = child
            .wait_with_output()
            .with_context(|| format!("failed to wait for {}", command.program))?;
        Ok(SecurityOutput {
            status: output.status.code().unwrap_or(1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// Create or replace a generic password. `-U` updates an existing item.
/// No `-A`: the ACL trusts `/usr/bin/security`, not every app.
pub(crate) fn store(
    runner: &impl SecurityRunner,
    service: &str,
    account: &str,
    secret: &str,
) -> Result<()> {
    let command = add_command(service, account, secret)?;
    let output = runner.run(&command)?;
    interpret_status(&output, "store a Keychain item")
}

/// Read a generic password. `Ok(None)` when the item does not exist.
pub(crate) fn get(
    runner: &impl SecurityRunner,
    service: &str,
    account: &str,
) -> Result<Option<String>> {
    let command = find_command(service, account)?;
    let output = runner.run(&command)?;
    interpret_find(&output)
}

/// Delete a generic password. Missing items are success.
pub(crate) fn delete(runner: &impl SecurityRunner, service: &str, account: &str) -> Result<()> {
    let command = delete_command(service, account)?;
    let output = runner.run(&command)?;
    if item_not_found(&output) {
        return Ok(());
    }
    interpret_status(&output, "delete a Keychain item")
}

fn add_command(service: &str, account: &str, secret: &str) -> Result<SecurityCommand> {
    let line = format!(
        "add-generic-password -U -s {} -a {} -w {}",
        quote_arg(service)?,
        quote_arg(account)?,
        quote_arg(secret)?
    );
    interactive_command(&line)
}

fn find_command(service: &str, account: &str) -> Result<SecurityCommand> {
    let line = format!(
        "find-generic-password -s {} -a {} -w",
        quote_arg(service)?,
        quote_arg(account)?
    );
    interactive_command(&line)
}

fn delete_command(service: &str, account: &str) -> Result<SecurityCommand> {
    let line = format!(
        "delete-generic-password -s {} -a {}",
        quote_arg(service)?,
        quote_arg(account)?
    );
    interactive_command(&line)
}

fn interactive_command(line: &str) -> Result<SecurityCommand> {
    let stdin = format!("{line}\n");
    if stdin.len() > INTERACTIVE_LINE_MAX {
        bail!(
            "cannot pass this command to {SECURITY_PATH}: it exceeds the tool's \
             4096-byte interactive command buffer"
        );
    }
    Ok(SecurityCommand {
        program: SECURITY_PATH,
        args: vec!["-i".to_string(), "-q".to_string()],
        stdin,
    })
}

/// Quote one argument the way `security -i`'s `split_line` unquotes it.
///
/// Double quotes, with `\` and `"` escaped. Newlines and NULs cannot be
/// carried: the parser splits on lines and on C strings.
fn quote_arg(value: &str) -> Result<String> {
    if value
        .chars()
        .any(|ch| ch == '\n' || ch == '\r' || ch == '\0')
    {
        bail!(
            "cannot pass this value to {SECURITY_PATH}: it contains a newline or NUL, \
             which the interactive parser cannot carry"
        );
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        if ch == '\\' || ch == '"' {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    Ok(quoted)
}

fn interpret_find(output: &SecurityOutput) -> Result<Option<String>> {
    if item_not_found(output) {
        return Ok(None);
    }
    interpret_status(output, "read a Keychain item")?;
    let mut stdout = output.stdout.clone();
    if stdout.last() == Some(&b'\n') {
        stdout.pop();
    }
    let secret = String::from_utf8(stdout)
        .context("Keychain item returned by /usr/bin/security was not valid UTF-8")?;
    Ok(Some(secret))
}

fn interpret_status(output: &SecurityOutput, action: &str) -> Result<()> {
    if output.status == 0 {
        return Ok(());
    }
    Err(security_failure(action, output))
}

fn item_not_found(output: &SecurityOutput) -> bool {
    if output.status == 0 {
        return false;
    }
    output.status == EXIT_ITEM_NOT_FOUND
        || stderr_text(output).contains("could not be found in the keychain")
}

fn security_failure(action: &str, output: &SecurityOutput) -> anyhow::Error {
    let stderr = stderr_text(output);
    if output.status == EXIT_INTERACTION_NOT_ALLOWED
        || stderr.contains("User interaction is not allowed")
    {
        return anyhow::anyhow!(
            "macOS Keychain refused to {action} without a prompt. This build uses \
             {SECURITY_PATH}, and an item created by another cx binary can still be locked \
             to that binary. Run `cx profiles add` again to store the secret, or set \
             CX_API_KEY and CX_REGION."
        );
    }
    if stderr.is_empty() {
        anyhow::anyhow!(
            "{SECURITY_PATH} failed to {action} (exit {})",
            output.status
        )
    } else {
        anyhow::anyhow!(
            "{SECURITY_PATH} failed to {action} (exit {}): {stderr}",
            output.status
        )
    }
}

fn stderr_text(output: &SecurityOutput) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct ScriptedRunner {
        calls: Mutex<Vec<SecurityCommand>>,
        responses: Mutex<Vec<SecurityOutput>>,
    }

    impl ScriptedRunner {
        fn new(responses: Vec<SecurityOutput>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(responses),
            }
        }

        fn calls(&self) -> Vec<SecurityCommand> {
            self.calls.lock().expect("runner lock").clone()
        }
    }

    impl SecurityRunner for ScriptedRunner {
        fn run(&self, command: &SecurityCommand) -> Result<SecurityOutput> {
            self.calls
                .lock()
                .expect("runner lock")
                .push(command.clone());
            let mut responses = self.responses.lock().expect("runner lock");
            if responses.is_empty() {
                bail!("no scripted security response left");
            }
            Ok(responses.remove(0))
        }
    }

    fn ok_output(stdout: &str) -> SecurityOutput {
        SecurityOutput {
            status: 0,
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn backend_is_security_cli_only_for_unsigned_macos() {
        assert_eq!(
            select_backend(true, false),
            Backend::SecurityCli,
            "unsigned or ad-hoc macOS builds shell out to /usr/bin/security"
        );
        assert_eq!(
            select_backend(true, true),
            Backend::NativeKeyring,
            "Developer ID-signed builds keep the direct Keychain ACL"
        );
        assert_eq!(select_backend(false, false), Backend::NativeKeyring);
        assert_eq!(select_backend(false, true), Backend::NativeKeyring);
    }

    #[test]
    fn store_sends_the_secret_on_stdin_not_in_args() {
        let secret = r#"p a\s"s word"#;
        let runner = ScriptedRunner::new(vec![ok_output("")]);
        store(&runner, "cx-cli:prod", "api_key", secret).unwrap();

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        let command = &calls[0];
        assert_eq!(command.program, SECURITY_PATH);
        assert_eq!(command.args, ["-i", "-q"]);
        assert!(
            !command.args.iter().any(|arg| arg.contains(secret)),
            "secret must not be a process argument"
        );
        let parsed = split_security_line(command.stdin.trim_end_matches('\n'));
        assert_eq!(
            parsed,
            vec![
                "add-generic-password".to_string(),
                "-U".to_string(),
                "-s".to_string(),
                "cx-cli:prod".to_string(),
                "-a".to_string(),
                "api_key".to_string(),
                "-w".to_string(),
                secret.to_string(),
            ]
        );
    }

    #[test]
    fn store_rejects_a_newline_without_running_security() {
        let runner = ScriptedRunner::new(vec![]);
        let err = store(&runner, "cx-cli:prod", "api_key", "line\nbreak").unwrap_err();
        assert!(
            err.to_string().contains("newline"),
            "error should name the limitation, got: {err}"
        );
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn store_rejects_a_secret_that_does_not_fit_the_interactive_line() {
        let runner = ScriptedRunner::new(vec![]);
        let secret = "k".repeat(INTERACTIVE_LINE_MAX);
        let err = store(&runner, "cx-cli:prod", "api_key", &secret).unwrap_err();
        assert!(
            err.to_string().contains("interactive command buffer"),
            "error should name the line limit, got: {err}"
        );
        assert!(runner.calls().is_empty());
        assert!(
            !err.to_string().contains(&secret),
            "the secret must not be copied into the error"
        );
    }

    #[test]
    fn get_strips_one_trailing_newline_and_keeps_interior_spaces() {
        let runner = ScriptedRunner::new(vec![ok_output(" sec ret \n")]);
        let secret = get(&runner, "cx-cli:prod", "api_key").unwrap();
        assert_eq!(secret.as_deref(), Some(" sec ret "));
    }

    #[test]
    fn get_missing_item_is_none() {
        let runner = ScriptedRunner::new(vec![SecurityOutput {
            status: EXIT_ITEM_NOT_FOUND,
            stdout: Vec::new(),
            stderr: b"security: The specified item could not be found in the keychain.\n".to_vec(),
        }]);
        assert_eq!(get(&runner, "cx-cli:prod", "api_key").unwrap(), None);
    }

    #[test]
    fn get_not_found_text_is_none_even_with_another_status() {
        let runner = ScriptedRunner::new(vec![SecurityOutput {
            status: 1,
            stdout: Vec::new(),
            stderr: b"The specified item could not be found in the keychain.\n".to_vec(),
        }]);
        assert_eq!(get(&runner, "svc", "acct").unwrap(), None);
    }

    #[test]
    fn get_interaction_required_names_the_other_binary() {
        let runner = ScriptedRunner::new(vec![SecurityOutput {
            status: EXIT_INTERACTION_NOT_ALLOWED,
            stdout: Vec::new(),
            stderr: b"User interaction is not allowed.\n".to_vec(),
        }]);
        let err = get(&runner, "svc", "acct").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("cx profiles add"), "got: {message}");
        assert!(message.contains(SECURITY_PATH), "got: {message}");
    }

    #[test]
    fn delete_missing_item_is_success() {
        let runner = ScriptedRunner::new(vec![SecurityOutput {
            status: EXIT_ITEM_NOT_FOUND,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }]);
        delete(&runner, "cx-cli:prod", "api_key").unwrap();
        let command = &runner.calls()[0];
        assert_eq!(command.args, ["-i", "-q"]);
        assert!(command.stdin.contains("delete-generic-password"));
        assert!(!command.stdin.contains("-w"));
    }

    #[test]
    fn failed_store_includes_stderr_but_not_the_secret() {
        let secret = "super-secret-value";
        let runner = ScriptedRunner::new(vec![SecurityOutput {
            status: 1,
            stdout: Vec::new(),
            stderr: b"keychain is locked\n".to_vec(),
        }]);
        let err = store(&runner, "cx-cli:prod", "api_key", secret).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("keychain is locked"), "got: {message}");
        assert!(!message.contains(secret), "got: {message}");
    }

    /// Port of `split_line` in Apple's `security.c`. The quoting tests use it
    /// to check that a secret survives the parser `security -i` actually runs.
    fn split_security_line(line: &str) -> Vec<String> {
        #[derive(Clone, Copy, PartialEq)]
        enum State {
            SkipWs,
            Read,
            ReadEsc,
            Quoted,
            QuotedEsc,
        }

        let bytes = line.as_bytes();
        let mut args: Vec<Vec<u8>> = Vec::new();
        let mut current: Vec<u8> = Vec::new();
        let mut in_arg = false;
        let mut state = State::SkipWs;
        let mut quote = 0u8;
        let mut index = 0;
        while index < bytes.len() {
            let ch = bytes[index];
            if state == State::SkipWs {
                if ch.is_ascii_whitespace() {
                    index += 1;
                    continue;
                }
                if ch == b'"' || ch == b'\'' {
                    quote = ch;
                    state = State::Quoted;
                    in_arg = true;
                    current.clear();
                    index += 1;
                    continue;
                }
                state = State::Read;
                in_arg = true;
                current.clear();
            }
            if state == State::Read {
                if ch == b'\\' {
                    state = State::ReadEsc;
                    index += 1;
                    continue;
                } else if ch.is_ascii_whitespace() {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                    state = State::SkipWs;
                    index += 1;
                    continue;
                }
                current.push(ch);
                index += 1;
                continue;
            }
            if state == State::Quoted {
                if ch == b'\\' {
                    state = State::QuotedEsc;
                    index += 1;
                    continue;
                }
                if ch == quote {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                    state = State::SkipWs;
                    index += 1;
                    continue;
                }
                current.push(ch);
                index += 1;
                continue;
            }
            if state == State::ReadEsc || state == State::QuotedEsc {
                current.push(ch);
                state = if state == State::ReadEsc {
                    State::Read
                } else {
                    State::Quoted
                };
                index += 1;
                continue;
            }
        }
        if in_arg {
            args.push(current);
        }
        args.into_iter()
            .map(|arg| String::from_utf8(arg).expect("security args are text"))
            .collect()
    }
}
