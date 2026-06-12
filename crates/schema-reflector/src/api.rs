use std::fs;
use std::path::Path;

use crate::error::Result;

pub fn write_api_code(output_dir: impl AsRef<Path>, template: &str) -> Result<()> {
    let output_dir = output_dir.as_ref();
    let api_dir = output_dir.join("api");
    fs::create_dir_all(&api_dir)?;
    fs::write(api_dir.join("mod.rs"), template)?;
    Ok(())
}
