# TOK Security Layer

TOK includes an optional local-first security/privacy layer that scans output for sensitive data and obfuscates it with deterministic placeholders before it reaches your LLM context.

## Design Philosophy

- **Never blocks**: The pipeline always continues. Sensitive data is obfuscated, not blocked.
- **Local-first**: All scanning and obfuscation happens on your machine. Nothing leaves your system unprotected.
- **Optional**: Security is disabled by default. Enable it when you need it.
- **Transparent**: Placeholders are restored automatically in responses.

## Enabling Security

### Via CLI Flag

```bash
tok proxy git status --security
tok proxy cargo test --security --security-mode strict
```

### Via Config

Add to `~/.config/tok/config.toml`:

```toml
[security]
enabled = true
mode = "balanced"
```

### Disabling (Override)

```bash
tok proxy git log --no-security
```

## Security Modes

| Mode | Behavior |
|------|----------|
| `observe` | Scan and report only. No modification to text. |
| `balanced` | Obfuscate common PII and secrets per config. |
| `strict` | Obfuscate everything detected, regardless of config. |
| `developer` | Preserve code/stack traces/URLs. Obfuscate secrets and internal identifiers. |

## What Gets Detected

### PII (regex-based)

- Email addresses
- Phone numbers
- URLs (http/https)
- Internal hostnames (*.internal, *.local, *.corp)
- IP addresses (IPv4, excluding localhost)
- Money values ($, €, £, etc.)

### Secrets (pattern-based)

- API keys (Stripe, GitHub PAT, AWS, OpenAI, Slack)
- JWT tokens
- Private key blocks (RSA, EC, OPENSSH)
- Password assignments
- Database URLs (postgres://, mysql://, mongodb://, redis://)
- Credit card numbers (Luhn-validated)

## Obfuscation Format

Detected values are replaced with deterministic placeholders:

```
{{TOK_EMAIL_001}}
{{TOK_SECRET_001}}
{{TOK_HOST_001}}
{{TOK_MONEY_001}}
```

The mapping between placeholders and original values exists only in memory and is never written to disk or sent externally.

## Configuration Reference

```toml
[security]
enabled = false
mode = "balanced"  # observe | balanced | strict | developer

[security.scan]
deterministic = true  # regex + pattern-based scanning
slm = false           # optional SLM semantic scanning

[security.actions]
# Per-entity-type action: "placeholder" or "allow"
email = "placeholder"
phone = "placeholder"
person = "placeholder"
company = "placeholder"
client = "placeholder"
internal_project = "placeholder"
url = "placeholder"
hostname = "placeholder"
ip_address = "placeholder"
money = "placeholder"
api_key = "placeholder"
jwt = "placeholder"
private_key = "placeholder"
password = "placeholder"
database_url = "placeholder"
credit_card = "placeholder"
bank_account = "placeholder"

[security.restore]
enabled = true
exact = true
validate_with_slm = false

[security.logging]
store_original_prompts = false
store_sanitized_prompts = true
redact_logs = true
```

## Mode Details

### Observe Mode

Scans and reports findings without modifying any text. Use this to understand what TOK would detect before enabling obfuscation:

```bash
tok --security --security-mode observe proxy echo "test@example.com"
# Output is unchanged; report shows what was detected
```

### Balanced Mode (Default)

Obfuscates entities according to the per-entity-type config. Most common choice for everyday use:

```bash
tok --security proxy echo "Email john@corp.com about $45,000 invoice"
# john@corp.com → {{TOK_EMAIL_001}}
# $45,000 → {{TOK_MONEY_001}}
```

### Strict Mode

Obfuscates all detected entities regardless of per-entity config. Use for maximum protection:

```bash
tok --security --security-mode strict proxy echo "Visit https://example.com"
# https://example.com → {{TOK_URL_001}} (even if config says "allow" for URLs)
```

### Developer Mode

Designed for debugging workflows. Preserves technical context (URLs, file paths, stack traces) while still obfuscating secrets and internal identifiers:

```bash
tok --security --security-mode developer proxy echo "Error at https://docs.rs/tokio key=sk_test_FAKE123"
# https://docs.rs/tokio → preserved (URL, useful for debugging)
# sk_test_FAKE123 → {{TOK_SECRET_001}} (API key, always obfuscated)
```

Developer mode obfuscates: API keys, JWTs, private keys, passwords, database URLs, credit cards, hostnames, IPs.
Developer mode preserves: URLs, money values, file paths, code snippets, stack traces.

## Inspecting Without Modifying

Use the inspect command to see what would be detected without modifying any output:

```bash
tok security-inspect ./prompt.txt --report
```

Supports stdin:
```bash
echo "postgres://admin:secret@prod.internal:5432/db" | tok security-inspect - --report
```

Example output:
```
TOK Security Inspect

  Mode:     balanced
  Findings: 1
  Severity: Critical

  1. [databaseurl] "***REDACTED***" (confidence: 95%, action: Placeholder)
```

The inspect command never prints actual sensitive values — they are shown as `***REDACTED***`.

## CLI Flags

| Flag | Purpose |
|------|---------|
| `--security` | Enable security layer for this invocation |
| `--no-security` | Disable security (overrides config) |
| `--security-mode <mode>` | Set mode: observe, balanced, strict, developer |
| `--slm` | Enable local SLM for semantic scanning |
| `--no-slm` | Disable SLM (overrides config) |

## Security Report

When verbose mode is active (`-v`), TOK prints a security summary:

```
TOK Security Report

  Security: enabled
  Mode:     balanced
  Risk:     high

  Obfuscated: 4 sensitive values
    - apikey: 1
    - email: 2
    - hostname: 1
```

## Commands Reference

### `tok security-inspect`

Scan a file or stdin for sensitive data without modifying anything. This is a dry-run mode for understanding what the security layer would detect.

**Usage:**
```bash
tok security-inspect <file>           # Scan a file
tok security-inspect <file> --report  # Show detailed report with severity
echo "text" | tok security-inspect -  # Scan from stdin
```

**Flags:**
- `--report` — Show entity-level detail (type, confidence, action)
- Respects global `--security-mode` flag for mode context

**Exit code:** Always 0 (never blocks).

### `tok doctor --slm`

Diagnose the local SLM runtime. Checks that the llama-server binary is available on the system, and that the configured model file exists.

**Usage:**
```bash
tok doctor --slm
```

**What it checks:**
1. `llama-server` binary in PATH or known install locations
2. Model file at configured path (`[slm] model_path`)
3. If both are present, attempts to start the server and verify health

**Example output (healthy):**
```
SLM Doctor

  Binary: /usr/local/bin/llama-server (found)
  Model:  ./models/tok-security-slm/model.gguf (found, 2.5 GB)
  Health: OK (started in 1.2s)
```

**Example output (not configured):**
```
SLM Doctor

  Binary: NOT FOUND — install llama.cpp or set PATH
  Model:  NOT FOUND — download model and set [slm].model_path
```

## Limitations

- Detection is regex/pattern-based for MVP. Some entities may be missed.
- Person names, company names, and other semantic entities require SLM mode.
- Streaming output is displayed in real-time; security applies only to tracked/captured text.
- The obfuscation map lives only in the current process memory.
- Credit card detection uses Luhn validation to reduce false positives on random digit sequences.
- JWT detection requires three valid base64url segments separated by dots.
