//! Resolves the platform-specific llama-server binary path.

use anyhow::Result;
use std::path::PathBuf;

/// Resolve the llama-server binary: check PATH first, then known install locations.
pub fn resolve() -> Result<PathBuf> {
    // Check PATH
    if let Ok(path) = which::which("llama-server") {
        return Ok(path);
    }

    // Check common install locations
    let candidates = if cfg!(target_os = "macos") {
        vec![
            "/usr/local/bin/llama-server",
            "/opt/homebrew/bin/llama-server",
        ]
    } else if cfg!(target_os = "linux") {
        vec!["/usr/local/bin/llama-server", "/usr/bin/llama-server"]
    } else {
        vec![]
    };

    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "llama-server binary not found.\n\
         Install llama.cpp or set the binary path in config.\n\
         See: https://github.com/ggerganov/llama.cpp"
    )
}

/// Check if llama-server is available without failing.
pub fn is_available() -> bool {
    resolve().is_ok()
}
