use std::io::IsTerminal;

use anyhow::{bail, Result};
use inquire::Confirm;

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
