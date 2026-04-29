use anyhow::{bail, Result};
use inquire::Confirm;

pub fn confirm_destructive(action: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    let confirmed = Confirm::new(action).with_default(false).prompt()?;
    if !confirmed {
        bail!("Cancelled.");
    }
    Ok(())
}
