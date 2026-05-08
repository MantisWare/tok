# TOK SLM Runtime

TOK can optionally use a local Small Language Model (SLM) via embedded llama.cpp for semantic sensitive data detection that goes beyond regex patterns.

## Overview

The SLM provides:
- Semantic entity detection (person names, company names, internal projects)
- Context-aware risk classification
- Restoration validation

The SLM is **optional** and **disabled by default**. Deterministic scanners (regex + pattern) always run first and their findings take precedence.

## Requirements

1. **llama-server binary** - from the llama.cpp project
2. **A GGUF model file** - quantized model for inference

## Installing llama.cpp

### macOS (Homebrew)

```bash
brew install llama.cpp
```

### From Source

```bash
git clone https://github.com/ggerganov/llama.cpp
cd llama.cpp
make -j
# Binary at: ./llama-server
```

### Verify Installation

```bash
which llama-server
# or
tok doctor --slm
```

## Recommended Models

| Model | Size | Notes |
|-------|------|-------|
| Qwen3-4B-Instruct Q4_K_M | ~2.5 GB | Recommended default. Good balance of speed and accuracy. |
| Phi-4-mini-instruct Q4_K_M | ~2.3 GB | Alternative. Fast, good at structured output. |

Download from Hugging Face and place at the configured path.

## Configuration

Add to `~/.config/tok/config.toml`:

```toml
[slm]
enabled = true
runtime = "embedded-llamacpp"
model_path = "./models/tok-security-slm/model.gguf"
context_size = 8192
temperature = 0.1
max_tokens = 1200
startup_timeout_ms = 30000
bind_host = "127.0.0.1"
```

### Configuration Options

| Key | Default | Description |
|-----|---------|-------------|
| `enabled` | `false` | Enable SLM scanning |
| `runtime` | `embedded-llamacpp` | Runtime type |
| `model_path` | `./models/tok-security-slm/model.gguf` | Path to GGUF model |
| `context_size` | `8192` | Context window size in tokens |
| `temperature` | `0.1` | Low temperature for deterministic classification |
| `max_tokens` | `1200` | Maximum response tokens |
| `startup_timeout_ms` | `30000` | Max time to wait for server startup |
| `bind_host` | `127.0.0.1` | Bind address (always localhost) |

## Usage

```bash
# Enable SLM for this invocation
tok proxy git status --security --slm

# Check SLM health
tok doctor --slm
```

## How It Works

1. TOK starts `llama-server` as a child process bound to `127.0.0.1` on a random port
2. Waits for the health endpoint to respond
3. Sends a JSON-only prompt asking the SLM to identify sensitive entities
4. Merges SLM findings with deterministic scanner results
5. Deterministic findings always take precedence over SLM
6. Server is stopped when the command completes

## Security Properties

- The SLM server binds **only to localhost** -- no network exposure
- The model runs entirely on your machine -- no data leaves your system
- SLM findings are advisory -- deterministic rules always win
- The server is ephemeral -- started per-command, stopped on exit

## Diagnostics

```bash
tok doctor --slm
```

Output:

```
TOK SLM Doctor

  llama-server binary: found (/opt/homebrew/bin/llama-server)
  Model file: found (./models/tok-security-slm/model.gguf, 2.5 GB)

  Configuration:
    Runtime:    embedded-llamacpp
    Context:    8192 tokens
    Temp:       0.1
    Max tokens: 1200
    Bind:       127.0.0.1
    Timeout:    30000ms

  All checks passed. SLM is ready.
```

## Troubleshooting

### "llama-server binary not found"

Install llama.cpp or ensure `llama-server` is in your PATH.

### "SLM model file not found"

Download a GGUF model and update `model_path` in config.

### "SLM runtime failed to start within 30000ms"

- Increase `startup_timeout_ms` in config
- Check system resources (RAM, disk)
- Try a smaller quantized model (Q4_K_S instead of Q4_K_M)
