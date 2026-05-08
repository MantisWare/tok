//! llama.cpp runtime manager: spawn, health-check, complete, shutdown.

use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::binary_resolver;
use super::model_resolver;
use crate::security::config::SlmConfig;

/// Manages the lifecycle of a local llama-server process.
pub struct LlamaCppRuntime {
    child: Option<Child>,
    port: u16,
    config: SlmConfig,
}

impl LlamaCppRuntime {
    pub fn new(config: &SlmConfig) -> Self {
        Self {
            child: None,
            port: 0,
            config: config.clone(),
        }
    }

    /// Start the llama-server process. Waits for health check before returning.
    pub fn start(&mut self) -> Result<()> {
        if self.child.is_some() {
            return Ok(());
        }

        let binary = binary_resolver::resolve()?;
        let model = model_resolver::resolve(&self.config.model_path)?;
        let port = find_available_port()?;

        let child = Command::new(&binary)
            .args([
                "--model",
                model.to_str().unwrap_or("model.gguf"),
                "--host",
                &self.config.bind_host,
                "--port",
                &port.to_string(),
                "--ctx-size",
                &self.config.context_size.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to start llama-server")?;

        self.child = Some(child);
        self.port = port;

        self.wait_for_health()?;
        Ok(())
    }

    /// Stop the llama-server process cleanly.
    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Health check: GET /health returns 200.
    pub fn health_check(&self) -> bool {
        if self.port == 0 {
            return false;
        }
        let url = format!("http://{}:{}/health", self.config.bind_host, self.port);
        ureq::get(&url).call().is_ok()
    }

    /// Send a JSON completion request to the running server.
    pub fn complete_json(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        if self.port == 0 {
            anyhow::bail!("SLM runtime is not running");
        }

        let url = format!("http://{}:{}/completion", self.config.bind_host, self.port);
        let prompt = format!(
            "<|system|>\n{}\n<|user|>\n{}\n<|assistant|>\n",
            system_prompt, user_prompt
        );

        let payload = serde_json::json!({
            "prompt": prompt,
            "temperature": self.config.temperature,
            "n_predict": self.config.max_tokens,
        });

        let response = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
            .context("SLM completion request failed")?;

        let body_str = response
            .into_string()
            .context("Failed to read SLM response")?;
        let body: serde_json::Value =
            serde_json::from_str(&body_str).context("Failed to parse SLM response JSON")?;
        let content = body["content"]
            .as_str()
            .or_else(|| body["response"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(content)
    }

    fn wait_for_health(&self) -> Result<()> {
        let timeout = Duration::from_millis(self.config.startup_timeout_ms);
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                anyhow::bail!(
                    "SLM runtime failed to start within {}ms",
                    self.config.startup_timeout_ms
                );
            }
            if self.health_check() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for LlamaCppRuntime {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Find an available TCP port on localhost.
fn find_available_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .context("Failed to bind to random port for SLM")?;
    let port = listener.local_addr()?.port();
    Ok(port)
}
