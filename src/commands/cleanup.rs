use anyhow::Result;

use crate::config::load_config;
use crate::spill::cleanup_old_files;

/// Remove all `cx_results*` files older than 30 minutes from the configured
/// temp directory.
pub fn run() -> Result<()> {
    let config = load_config().unwrap_or_default();
    let temp_dir = &config.temp_dir;

    let removed = cleanup_old_files(temp_dir)?;
    if removed == 0 {
        println!("No stale result files found in {temp_dir}.");
    } else {
        println!("Removed {removed} stale result file(s) from {temp_dir}.");
    }
    Ok(())
}
