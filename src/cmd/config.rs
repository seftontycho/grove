use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::config::Config;

pub fn show() -> Result<()> {
    let path = Config::path()?;
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    println!("{}", path.display());
    println!("---");
    print!("{contents}");
    Ok(())
}

pub fn edit() -> Result<()> {
    let path = Config::path()?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| format!("Failed to open {editor}"))?;

    if !status.success() {
        bail!("{editor} exited with non-zero status");
    }

    // Validate the config after editing
    match Config::load() {
        Ok(_) => println!("Config saved."),
        Err(e) => println!("Warning: config may be invalid: {e}"),
    }

    Ok(())
}
