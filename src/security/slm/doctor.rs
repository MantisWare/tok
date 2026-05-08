//! `tok doctor --slm` health checks for the SLM runtime.

use anyhow::Result;
use colored::Colorize;

use super::binary_resolver;
use super::model_resolver;
use crate::security::config::SlmConfig;

/// Run all SLM diagnostics and print results.
pub fn run(config: &SlmConfig) -> Result<()> {
    println!("{}", "TOK SLM Doctor".bold());
    println!();

    let mut all_ok = true;

    // Check 1: llama-server binary
    print!("  llama-server binary: ");
    match binary_resolver::resolve() {
        Ok(path) => println!("{} ({})", "found".green(), path.display()),
        Err(_) => {
            println!("{}", "NOT FOUND".red());
            println!("    Install llama.cpp: https://github.com/ggerganov/llama.cpp");
            all_ok = false;
        }
    }

    // Check 2: Model file
    print!("  Model file: ");
    match model_resolver::resolve(&config.model_path) {
        Ok(path) => {
            let size = std::fs::metadata(&path)
                .map(|m| format_bytes(m.len()))
                .unwrap_or_else(|_| "unknown size".into());
            println!("{} ({}, {})", "found".green(), path.display(), size);
        }
        Err(_) => {
            println!("{}", "NOT FOUND".red());
            println!("    Expected at: {}", config.model_path.display());
            println!("    Recommended: Qwen3-4B-Instruct GGUF Q4_K_M");
            all_ok = false;
        }
    }

    // Check 3: Config summary
    println!();
    println!("  Configuration:");
    println!("    Runtime:    {}", config.runtime);
    println!("    Context:    {} tokens", config.context_size);
    println!("    Temp:       {}", config.temperature);
    println!("    Max tokens: {}", config.max_tokens);
    println!("    Bind:       {}", config.bind_host);
    println!("    Timeout:    {}ms", config.startup_timeout_ms);

    println!();
    if all_ok {
        println!("  {}", "All checks passed. SLM is ready.".green());
    } else {
        println!(
            "  {}",
            "Some checks failed. Fix the issues above to use --slm.".yellow()
        );
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", bytes / 1024)
    }
}
