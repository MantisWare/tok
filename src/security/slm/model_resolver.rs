//! Resolves the .gguf model file path for the SLM runtime.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Resolve the model path: check configured path, then default locations.
pub fn resolve(configured_path: &Path) -> Result<PathBuf> {
    // If configured path is absolute and exists, use it
    if configured_path.is_absolute() && configured_path.exists() {
        return Ok(configured_path.to_path_buf());
    }

    // Try relative to current directory
    if configured_path.exists() {
        return Ok(configured_path
            .canonicalize()
            .unwrap_or(configured_path.to_path_buf()));
    }

    // Try relative to TOK data directory
    if let Some(data_dir) = dirs::data_dir() {
        let tok_model = data_dir.join("tok").join(configured_path);
        if tok_model.exists() {
            return Ok(tok_model);
        }
    }

    // Try the default model directory
    let default_path = PathBuf::from("models/tok-security-slm/model.gguf");
    if default_path.exists() {
        return Ok(default_path);
    }

    anyhow::bail!(
        "SLM model file not found: {}\n\
         Place a .gguf model at the configured path or update [slm] model_path in config.toml.\n\
         Recommended: Qwen3-4B-Instruct Q4_K_M",
        configured_path.display()
    )
}

/// Check if a model file is available at the configured path.
pub fn is_available(configured_path: &Path) -> bool {
    resolve(configured_path).is_ok()
}
