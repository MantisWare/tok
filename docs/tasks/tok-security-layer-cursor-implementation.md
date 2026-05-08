# TOK Security Layer Implementation Plan

## Project

**TOK — Token Optimization Kit**

Existing repository:

```text
git@github.com-mantis:MantisWare/tok.git
```

Existing product positioning:

> TOK is a CLI/proxy that shaves 60–90% off the tokens your LLM eats — same commands, less wallpaper.

This document describes how to extend TOK with an optional local-first security layer inspired by Prompt-Shield.

The goal is to add:

- Sensitive-data scanning
- PII and secret detection
- Obfuscation before provider calls
- Response restoration after provider calls
- Optional local SLM review
- Embedded llama.cpp runtime support
- Config-driven enablement
- Minimal disruption to existing TOK CLI/proxy flows

---

## 1. Desired Outcome

Add a security/privacy layer to TOK that can sit in the existing prompt-processing pipeline.

TOK should continue to do token optimization as it already does, but optionally also:

1. Intercept the original prompt.
2. Scan for sensitive data.
3. Block dangerous secrets when configured.
4. Replace sensitive values with placeholders or fake data.
5. Optimize the sanitized prompt.
6. Send only the sanitized prompt to the target LLM provider.
7. Intercept the LLM response.
8. Restore original values locally.
9. Return the restored response to the user.
10. Report token savings and security actions.

The security layer should be **off by default** so existing TOK behavior is preserved.

Recommended default:

```yaml
security:
  enabled: false
```

---

## 2. Product Direction

TOK should become:

> A token optimization proxy with optional local privacy and prompt security controls.

Suggested expanded tagline:

> TOK compresses prompts, strips waste, and optionally shields sensitive data before your LLM ever sees it.

---

## 3. Core Design Principle

Do not replace TOK’s current optimization pipeline.

Instead, add a configurable pre/post-processing security pipeline:

```text
Current TOK flow:

User / App / CLI
      |
      v
TOK optimization
      |
      v
LLM provider
      |
      v
Response
```

Enhanced optional flow:

```text
User / App / CLI
      |
      v
TOK input interceptor
      |
      v
Security scanner
      |
      v
Policy evaluator
      |
      v
Obfuscation engine
      |
      v
Existing TOK optimizer
      |
      v
LLM provider
      |
      v
Response interceptor
      |
      v
Restoration engine
      |
      v
Final response
```

Important:

- The existing optimizer should continue to work.
- Security should be an optional pipeline stage.
- The obfuscation map must never leave the local machine.
- The local SLM should assist, not replace deterministic security rules.

---

## 4. Feature Scope

### 4.1 MVP Security Features

Implement first:

- Config flag to enable/disable security
- Regex-based PII scanner
- Secret scanner
- Policy evaluator
- Placeholder obfuscation
- Memory-only obfuscation map
- Response restoration
- Dry-run inspection mode
- Security report in CLI output
- Tests proving sensitive values are not sent to provider adapter mocks

### 4.2 Optional SLM Features

Add behind config:

- Embedded llama.cpp runtime
- Bundled or user-provided `.gguf` SLM model
- SLM semantic scanner
- SLM prompt optimization review
- SLM restoration validation
- Runtime doctor command

### 4.3 Future Features

- Fake data replacement
- Hybrid fake-data plus placeholder mode
- Encrypted local vault
- Team policy packs
- GUI dashboard
- MCP server integration
- Streaming-safe restoration
- Security event analytics

---

## 5. Configuration

Add security config to TOK’s existing configuration system.

If TOK already has a config file, extend it. If not, add support for:

```text
tok.config.yaml
```

Example config:

```yaml
optimization:
  enabled: true
  mode: balanced

security:
  enabled: false
  mode: balanced

  scan:
    deterministic: true
    slm: false

  actions:
    email: placeholder
    phone: placeholder
    person: placeholder
    company: placeholder
    client: placeholder
    internal_project: placeholder
    url: placeholder
    hostname: placeholder
    ip_address: placeholder
    money: placeholder
    api_key: block
    jwt: block
    private_key: block
    password: block
    database_url: block
    credit_card: block
    bank_account: block

  restore:
    enabled: true
    exact: true
    aliases: false
    fuzzy: false
    validate_with_slm: false

  logging:
    store_original_prompts: false
    store_sanitized_prompts: true
    redact_logs: true

slm:
  enabled: false
  runtime: embedded-llamacpp
  model_path: ./models/tok-security-slm/model.gguf
  context_size: 8192
  temperature: 0.1
  max_tokens: 1200
  startup_timeout_ms: 30000
  bind_host: 127.0.0.1
```

---

## 6. CLI Flags

Add CLI flags that can override config.

Recommended flags:

```bash
tok --security
tok --no-security
tok --security-mode strict
tok --security-mode balanced
tok --security-mode developer
tok --security-inspect
tok --security-report
tok --slm
tok --no-slm
tok doctor
tok models verify
```

Examples:

```bash
tok run ./prompt.txt --security
```

```bash
tok proxy start --security
```

```bash
tok inspect ./prompt.txt --security-report
```

```bash
tok run ./prompt.txt --security --slm
```

```bash
tok doctor --slm
```

---

## 7. Security Modes

### 7.1 Off

```yaml
security:
  enabled: false
```

Behavior:

- No security scanning.
- Existing TOK behavior remains unchanged.

### 7.2 Observe

```yaml
security:
  enabled: true
  mode: observe
```

Behavior:

- Scan prompt.
- Produce security report.
- Do not modify prompt.
- Do not block.
- Useful for onboarding.

### 7.3 Balanced

```yaml
security:
  enabled: true
  mode: balanced
```

Behavior:

- Obfuscate common PII.
- Block obvious secrets.
- Restore response.
- Recommended default once security is enabled.

### 7.4 Strict

```yaml
security:
  enabled: true
  mode: strict
```

Behavior:

- Obfuscate most detected sensitive entities.
- Block all high-risk secret classes.
- Disable fake-data mode initially.
- Prefer deterministic placeholders.

### 7.5 Developer

```yaml
security:
  enabled: true
  mode: developer
```

Behavior:

- Preserve code blocks, stack traces, filenames, package versions, and line numbers.
- Obfuscate secrets, tokens, internal hostnames, customer identifiers, and URLs.
- Avoid damaging technical debugging context.

---

## 8. Pipeline Integration

Cursor should inspect the existing TOK codebase first and locate:

- CLI entrypoints
- Proxy server request handler
- Existing prompt optimizer
- Existing provider adapters
- Existing tokenizer/cost estimator
- Existing config loader
- Existing logging system
- Existing test framework

Then add security stages around the existing optimizer.

Recommended internal pipeline:

```ts
export async function processTokRequest(input: TokRequest): Promise<TokResponse> {
  const config = await loadTokConfig(input);

  const securityEnabled = resolveSecurityEnabled(config, input.flags);

  let workingPrompt = input.prompt;
  let securityContext: SecurityContext | undefined;

  if (securityEnabled) {
    const securityInputResult = await processSecurityInput({
      prompt: workingPrompt,
      config,
      requestId: input.requestId,
    });

    if (securityInputResult.decision.action === "block") {
      throw new TokSecurityBlockedError(securityInputResult.decision.reason);
    }

    workingPrompt = securityInputResult.sanitizedPrompt;
    securityContext = securityInputResult.securityContext;
  }

  const optimizedPrompt = await runExistingTokOptimizer({
    ...input,
    prompt: workingPrompt,
  });

  const providerResponse = await sendToProvider({
    ...input,
    prompt: optimizedPrompt.prompt,
  });

  let finalResponse = providerResponse.text;

  if (securityEnabled && securityContext?.obfuscationMap) {
    const restored = await processSecurityOutput({
      responseText: finalResponse,
      securityContext,
      config,
    });

    finalResponse = restored.text;
  }

  return {
    ...providerResponse,
    text: finalResponse,
    security: securityContext?.report,
    optimization: optimizedPrompt.report,
  };
}
```

Important:

- Run security scanning before optimization.
- Run obfuscation before optimization.
- Run existing TOK optimization on the sanitized prompt.
- Run restoration only after provider response.
- Make sure optimization does not destroy placeholder tokens like `{{TOK_EMAIL_001}}`.

---

## 9. Security Module Layout

Create a security module.

Suggested location, depending on current repo structure:

```text
src/security/
```

or:

```text
packages/security/
```

Recommended files:

```text
src/security/
  index.ts
  types.ts
  processSecurityInput.ts
  processSecurityOutput.ts

  scanner/
    scanPrompt.ts
    regexScanner.ts
    secretScanner.ts
    slmScanner.ts
    mergeFindings.ts
    detectors/
      email.ts
      phone.ts
      url.ts
      hostname.ts
      ipAddress.ts
      money.ts
      apiKey.ts
      jwt.ts
      privateKey.ts
      password.ts
      databaseUrl.ts
      creditCard.ts
      bankAccount.ts

  policy/
    evaluateSecurityPolicy.ts
    defaultSecurityPolicy.ts
    modes.ts

  obfuscation/
    obfuscatePrompt.ts
    restoreResponse.ts
    placeholderStrategy.ts
    fakeDataStrategy.ts
    mapStore.ts
    unresolvedPlaceholders.ts

  slm/
    localSlmClient.ts
    prompts.ts
    schemas.ts

  runtime/
    llamaCppRuntime.ts
    llamaCppBinaryResolver.ts
    modelResolver.ts
    runtimeDoctor.ts
```

---

## 10. Security Types

Add strongly typed interfaces.

```ts
export type SensitiveEntityType =
  | "person"
  | "email"
  | "phone"
  | "address"
  | "company"
  | "client"
  | "internal_project"
  | "url"
  | "hostname"
  | "ip_address"
  | "api_key"
  | "jwt"
  | "private_key"
  | "password"
  | "database_url"
  | "credit_card"
  | "bank_account"
  | "money"
  | "medical"
  | "legal"
  | "custom";

export type SecurityAction =
  | "allow"
  | "placeholder"
  | "fake"
  | "redact"
  | "block";

export interface SensitiveFinding {
  id: string;
  type: SensitiveEntityType;
  value: string;
  startIndex: number;
  endIndex: number;
  confidence: number;
  source: "regex" | "secret" | "slm" | "custom";
  recommendedAction: SecurityAction;
  reason?: string;
}

export interface ObfuscationMapEntry {
  id: string;
  type: SensitiveEntityType;
  original: string;
  replacement: string;
  placeholder: string;
  aliases: string[];
  action: SecurityAction;
  confidence: number;
}

export interface ObfuscationMap {
  requestId: string;
  createdAt: string;
  entries: ObfuscationMapEntry[];
  storageMode: "memory";
}

export interface SecurityDecision {
  action: "allow" | "modify" | "block";
  riskLevel: "none" | "low" | "medium" | "high" | "critical";
  reason?: string;
  warnings: string[];
}

export interface SecurityContext {
  requestId: string;
  findings: SensitiveFinding[];
  decision: SecurityDecision;
  obfuscationMap: ObfuscationMap;
  report: SecurityReport;
}

export interface SecurityReport {
  enabled: boolean;
  mode: string;
  riskLevel: SecurityDecision["riskLevel"];
  blocked: boolean;
  entityCounts: Record<string, number>;
  actions: Record<string, number>;
  warnings: string[];
}
```

---

## 11. Deterministic Detectors

Implement these before the SLM.

### 11.1 Email

Detect:

```text
john@example.com
john.smith+test@example.co.za
```

Action:

```text
placeholder
```

### 11.2 Phone

Detect international and local phone patterns.

Action:

```text
placeholder
```

### 11.3 URL

Detect:

```text
https://internal.example.com/path
http://localhost:3000/api
```

Action:

```text
placeholder
```

### 11.4 Hostname / Internal Hostname

Detect:

```text
db-prod-01.internal
staging-api.company.local
localhost
```

Action:

```text
placeholder
```

### 11.5 IP Address

Detect IPv4 and optionally IPv6.

Action:

```text
placeholder
```

### 11.6 API Keys

Detect common patterns:

```text
sk_live_...
sk_test_...
ghp_...
github_pat_...
xoxb-...
AKIA...
AIza...
```

Action:

```text
block by default
```

### 11.7 JWT

Detect three-part JWT-like values:

```text
xxxxx.yyyyy.zzzzz
```

Action:

```text
block by default
```

### 11.8 Private Key

Detect:

```text
-----BEGIN PRIVATE KEY-----
-----BEGIN RSA PRIVATE KEY-----
-----BEGIN OPENSSH PRIVATE KEY-----
```

Action:

```text
block by default
```

### 11.9 Password

Detect obvious password assignment patterns:

```text
password=...
PASSWORD=...
db_password: ...
```

Action:

```text
block by default
```

### 11.10 Database URL

Detect:

```text
postgres://user:pass@host:5432/db
mysql://user:pass@host/db
mongodb+srv://...
redis://...
```

Action:

```text
block by default
```

### 11.11 Credit Card-Like Values

Detect candidate card values and validate with Luhn check.

Action:

```text
block by default
```

### 11.12 Money Values

Detect:

```text
$45,000
USD 12000
€1,200.50
```

Action:

```text
placeholder
```

---

## 12. Finding Merge Logic

Multiple detectors may overlap.

Rules:

1. Prefer `private_key`, `password`, `database_url`, `jwt`, and `api_key` over all lower-risk types.
2. Prefer longer spans when entity type is the same.
3. Prefer higher confidence.
4. Preserve all source metadata.
5. Avoid duplicate placeholder replacements.

Example:

```text
postgres://admin:secret@prod-db.company.local:5432/app
```

Should be one `database_url` finding, not separate URL, hostname, password, and company findings.

---

## 13. Policy Evaluation

Implement `evaluateSecurityPolicy`.

Policy should decide:

- Allow prompt unchanged
- Modify prompt through obfuscation
- Block prompt entirely

Default high-risk blockers:

```text
private_key
password
database_url
jwt
api_key
credit_card
bank_account
```

In observe mode, do not block.

In balanced/strict/developer mode, block high-risk secrets by default.

Example blocked error:

```text
TOK blocked this request because a private key was detected. 
Run with --security-mode observe to inspect without sending, or update tok.config.yaml if this was intentional.
```

Do not print the secret value.

---

## 14. Obfuscation Strategy

Implement placeholder mode first.

### 14.1 Placeholder Format

Use deterministic placeholder format:

```text
{{TOK_EMAIL_001}}
{{TOK_PERSON_001}}
{{TOK_COMPANY_001}}
{{TOK_URL_001}}
{{TOK_SECRET_001}}
```

Important:

- The optimizer must preserve these tokens.
- Add optimizer tests to ensure placeholders are not changed.
- Avoid placeholder names that look like natural text.

### 14.2 Example

Original:

```text
Write an email to John Smith at john@example.com about the $45,000 ACME invoice.
```

Sanitized:

```text
Write an email to {{TOK_PERSON_001}} at {{TOK_EMAIL_001}} about the {{TOK_MONEY_001}} {{TOK_COMPANY_001}} invoice.
```

Map:

```json
{
  "requestId": "req_123",
  "entries": [
    {
      "id": "TOK_PERSON_001",
      "type": "person",
      "original": "John Smith",
      "replacement": "{{TOK_PERSON_001}}",
      "placeholder": "{{TOK_PERSON_001}}",
      "aliases": [],
      "action": "placeholder",
      "confidence": 0.82
    }
  ]
}
```

---

## 15. Restoration Strategy

After provider response, restore placeholders.

```ts
export function restoreResponse(
  responseText: string,
  map: ObfuscationMap
): RestorationResult {
  let text = responseText;

  for (const entry of map.entries) {
    text = text.split(entry.placeholder).join(entry.original);
    text = text.split(entry.replacement).join(entry.original);
  }

  return {
    text,
    unresolvedPlaceholders: findUnresolvedTokPlaceholders(text),
  };
}
```

MVP restoration should be deterministic only.

Do not add fuzzy restoration in MVP unless explicitly configured.

---

## 16. Integration with TOK Optimizer

The optimizer must preserve security placeholders.

Add a test like:

```ts
it("preserves TOK security placeholders during optimization", async () => {
  const input = "Please email {{TOK_PERSON_001}} at {{TOK_EMAIL_001}}.";
  const output = await optimize(input);
  expect(output).toContain("{{TOK_PERSON_001}}");
  expect(output).toContain("{{TOK_EMAIL_001}}");
});
```

Add a placeholder protection step if needed:

```text
Before optimization:
  Replace placeholders with protected sentinel tokens.

After optimization:
  Restore sentinel tokens.
```

Example sentinel:

```text
⟦TOK_PLACEHOLDER_001⟧
```

But prefer keeping the actual placeholders if the optimizer does not mutate them.

---

## 17. SLM Integration

SLM support should be optional.

Config:

```yaml
slm:
  enabled: false
  runtime: embedded-llamacpp
  model_path: ./models/tok-security-slm/model.gguf
```

CLI:

```bash
tok run ./prompt.txt --security --slm
```

The SLM should be used for:

- Semantic sensitive entity detection
- Prompt risk classification
- Optional optimization suggestions
- Restoration validation

The SLM should not be the final authority for blocking secrets.

Deterministic scanners and policy rules must win.

---

## 18. Recommended SLM

Recommended default custom SLM base model:

```text
Qwen3-4B-Instruct GGUF Q4_K_M
```

Alternative:

```text
Phi-4-mini-instruct GGUF Q4_K_M
```

Recommended model file location:

```text
models/tok-security-slm/model.gguf
```

Recommended runtime:

```text
llama.cpp llama-server
```

Recommended SLM settings:

```yaml
slm:
  temperature: 0.1
  context_size: 8192
  max_tokens: 1200
```

For deterministic classification, keep temperature low.

---

## 19. Embedded llama.cpp Runtime

Add embedded llama.cpp support.

Suggested files:

```text
src/security/runtime/
  llamaCppRuntime.ts
  llamaCppBinaryResolver.ts
  modelResolver.ts
  runtimeDoctor.ts
  jsonCompletion.ts
```

If the repo is monorepo-based, use:

```text
packages/runtime-llamacpp/
```

### 19.1 Runtime Behavior

The runtime should:

1. Resolve platform-specific `llama-server`.
2. Resolve the configured `.gguf` model.
3. Start llama.cpp as a child process.
4. Bind only to `127.0.0.1`.
5. Use a random available port by default.
6. Health check the server.
7. Send JSON-only prompts.
8. Parse JSON response.
9. Repair JSON if minor formatting issues occur.
10. Stop process cleanly after command completion unless daemon/proxy mode is running.

### 19.2 Supported Runtime Modes

```yaml
slm:
  runtime: embedded-llamacpp
```

Future optional modes:

```yaml
slm:
  runtime: external-ollama
```

```yaml
slm:
  runtime: external-llamacpp
```

```yaml
slm:
  runtime: disabled
```

### 19.3 Runtime Interface

```ts
export interface LocalSlmRuntime {
  start(): Promise<void>;
  stop(): Promise<void>;
  isRunning(): boolean;
  healthCheck(): Promise<boolean>;
  completeJson<T>(request: SlmJsonRequest): Promise<T>;
}

export interface SlmJsonRequest {
  systemPrompt: string;
  userPrompt: string;
  schemaName: string;
  maxTokens?: number;
  temperature?: number;
}

export interface LlamaCppRuntimeOptions {
  binaryPath?: string;
  modelPath: string;
  host: "127.0.0.1";
  port?: number;
  contextSize: number;
  temperature: number;
  maxTokens: number;
  startupTimeoutMs: number;
}
```

### 19.4 Runtime Implementation Skeleton

```ts
import { spawn, ChildProcessWithoutNullStreams } from "node:child_process";
import { once } from "node:events";

export class LlamaCppRuntime implements LocalSlmRuntime {
  private child?: ChildProcessWithoutNullStreams;
  private port?: number;

  constructor(private readonly options: LlamaCppRuntimeOptions) {}

  async start(): Promise<void> {
    if (this.child) return;

    const binaryPath = this.options.binaryPath ?? resolveBundledLlamaServerBinary();
    const modelPath = resolveModelPath(this.options.modelPath);
    const port = this.options.port ?? await findAvailablePort();

    this.port = port;

    this.child = spawn(binaryPath, [
      "--model", modelPath,
      "--host", "127.0.0.1",
      "--port", String(port),
      "--ctx-size", String(this.options.contextSize),
    ], {
      stdio: ["ignore", "pipe", "pipe"],
    });

    this.child.once("exit", () => {
      this.child = undefined;
    });

    await waitForLlamaHealth({
      host: "127.0.0.1",
      port,
      timeoutMs: this.options.startupTimeoutMs,
    });
  }

  async stop(): Promise<void> {
    if (!this.child) return;

    this.child.kill("SIGTERM");

    try {
      await once(this.child, "exit");
    } finally {
      this.child = undefined;
    }
  }

  isRunning(): boolean {
    return Boolean(this.child);
  }

  async healthCheck(): Promise<boolean> {
    if (!this.port) return false;

    try {
      const response = await fetch(`http://127.0.0.1:${this.port}/health`);
      return response.ok;
    } catch {
      return false;
    }
  }

  async completeJson<T>(request: SlmJsonRequest): Promise<T> {
    if (!this.port) {
      throw new Error("TOK SLM runtime is not running.");
    }

    const prompt = buildJsonOnlyPrompt(request);

    const response = await fetch(`http://127.0.0.1:${this.port}/completion`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        prompt,
        temperature: request.temperature ?? this.options.temperature,
        n_predict: request.maxTokens ?? this.options.maxTokens,
      }),
    });

    if (!response.ok) {
      throw new Error(`TOK SLM request failed with status ${response.status}.`);
    }

    const payload = await response.json();
    const rawText = payload.content ?? payload.response ?? "";

    return parseAndRepairJson<T>(rawText);
  }
}
```

---

## 20. SLM Scanner Prompt

Use a strict JSON-only prompt.

System prompt:

```text
You are TOK Security SLM.

Your task is to inspect user prompts for sensitive data that deterministic scanners may miss.

Return JSON only.

Do not include markdown.
Do not include explanations outside JSON.
Do not rewrite the original prompt.
Do not include chain-of-thought.
```

User prompt template:

```text
Analyze the following prompt for sensitive entities.

Return this JSON schema:

{
  "sensitive_entities": [
    {
      "type": "person | company | client | internal_project | medical | legal | custom",
      "value": "exact text span from prompt",
      "confidence": 0.0,
      "recommended_action": "allow | placeholder | fake | redact | block",
      "reason": "short reason"
    }
  ],
  "risk_level": "none | low | medium | high | critical",
  "safe_to_send": true,
  "warnings": []
}

Prompt:
---
{{PROMPT}}
---
```

Important:

- Values must be exact text spans.
- The SLM must not invent entities.
- Deterministic scanner findings should be merged afterward.
- Secret scanner findings override SLM recommendations.

---

## 21. SLM Restoration Validation Prompt

Optional.

System prompt:

```text
You are TOK Restoration Validator.

You inspect a restored LLM response and verify whether any TOK placeholders or fake replacement values remain unresolved.

Return JSON only.
```

Schema:

```json
{
  "restoration_status": "complete",
  "unresolved_placeholders": [],
  "warnings": []
}
```

---

## 22. CLI UX

### 22.1 Inspect Output

Example:

```text
TOK Security Report

Security: enabled
Mode: balanced
SLM: disabled

Risk: high
Action: blocked

Detected:
- email: 2
- api_key: 1
- company: 1
- money: 1

Blocked reason:
A high-risk API key was detected.

Token estimate:
Original: 2,411
After security obfuscation: 2,398
After TOK optimization: 824
Savings: 65.8%
```

Do not print sensitive values unless user explicitly passes a dangerous debug flag.

### 22.2 Debug Mode

If a debug flag exists, still redact sensitive values by default.

Do not add a flag that casually prints secrets.

If absolutely needed for local development:

```bash
tok inspect ./prompt.txt --security-debug-local-only
```

Require explicit scary naming.

---

## 23. Proxy Integration

For proxy mode:

```bash
tok proxy start --security
```

The proxy should apply the same input/output security pipeline.

Flow:

```text
HTTP request
      |
Normalize provider request
      |
Security input pipeline
      |
TOK optimizer
      |
Provider adapter
      |
Security output pipeline
      |
HTTP response
```

If streaming is currently supported by TOK, add a config option:

```yaml
security:
  streaming:
    mode: buffer
```

Recommended MVP behavior:

- If security enabled and streaming requested, buffer full response internally.
- Restore response after full provider output.
- Return non-streaming response or documented buffered stream behavior.

Reason:

- Real-time restoration of streaming chunks is hard because fake values/placeholders may be split across chunks.

---

## 24. Provider Safety Test

Add tests ensuring provider adapters never receive original secrets.

Example:

```ts
it("does not send original email or API key to provider when security is enabled", async () => {
  const prompt = "Email john@example.com using key sk_test_FAKE123.";

  const provider = createMockProvider();

  await runTokPipeline({
    prompt,
    provider,
    config: {
      security: {
        enabled: true,
        mode: "balanced",
      },
    },
  });

  const sentPrompt = provider.getLastPrompt();

  expect(sentPrompt).not.toContain("john@example.com");
  expect(sentPrompt).not.toContain("sk_test_FAKE123");
  expect(sentPrompt).toContain("{{TOK_EMAIL_001}}");
});
```

---

## 25. Tests to Add

### Scanner Tests

- detects email
- detects phone
- detects URL
- detects internal hostname
- detects IP address
- detects API key
- detects JWT
- detects private key
- detects password
- detects database URL
- detects credit card with Luhn
- detects money

### Policy Tests

- observe mode does not block
- balanced mode blocks secrets
- strict mode obfuscates more entities
- developer mode preserves technical context

### Obfuscation Tests

- placeholders are deterministic
- obfuscation map stores originals locally
- sanitized prompt excludes originals
- overlapping findings are handled correctly

### Restoration Tests

- placeholders restore exactly
- unresolved placeholders are reported
- restoration does not mutate unrelated text

### Optimizer Integration Tests

- optimizer preserves TOK placeholders
- optimizer does not remove security constraints
- optimizer does not reveal original values

### Proxy Tests

- proxy applies security before provider call
- proxy restores response
- proxy redacts logs
- streaming behavior is documented and tested

### Runtime Tests

- llama.cpp binary resolver selects correct platform
- missing model returns clear error
- runtime binds to localhost
- runtime starts and stops cleanly with mocked process
- `completeJson` parses valid JSON
- malformed JSON repair works for simple cases

---

## 26. Logging and Audit Rules

Default logs may include:

```text
request id
timestamp
security enabled
mode
risk level
entity counts
actions taken
blocked status
provider
model
token counts
savings percentage
```

Default logs must never include:

```text
original prompt
original sensitive values
private keys
passwords
API keys
database URLs
obfuscation map
restored response
```

Add a redaction utility used by all log paths.

---

## 27. Error Handling

Create clear errors.

```ts
export class TokSecurityBlockedError extends Error {
  constructor(
    public readonly reason: string,
    public readonly riskLevel: string
  ) {
    super(reason);
  }
}
```

Example output:

```text
TOK blocked this request.

Reason:
A private key was detected.

No data was sent to the provider.

Suggestion:
Remove the secret, use observe mode for local inspection, or change your security policy if this is intentional.
```

---

## 28. Documentation Updates

Update README with:

- What TOK security mode does
- How to enable it
- What data is scanned
- What gets blocked by default
- How SLM mode works
- How llama.cpp is embedded
- How to provide your own `.gguf` model
- How to use security in proxy mode
- Current streaming limitations
- Security disclaimers

Add docs:

```text
docs/SECURITY_LAYER.md
docs/SLM_RUNTIME.md
docs/CONFIG.md
docs/PROXY_SECURITY.md
```

---

## 29. Implementation Phases for Cursor

### Phase 1 — Repo Inspection and Plan

Cursor should first inspect the current TOK repo and identify:

- Main CLI entrypoint
- Main proxy handler
- Prompt optimization pipeline
- Provider adapter layer
- Config system
- Logging system
- Test framework

Then produce a short implementation note before coding.

### Phase 2 — Security Config and Types

Implement:

- Security config schema
- Default security config
- CLI flags
- Shared security types

Deliverable:

```bash
tok --security --help
```

or equivalent based on existing CLI.

### Phase 3 — Deterministic Scanner

Implement:

- Regex scanner
- Secret scanner
- Detector tests
- Finding merge logic

Deliverable:

```bash
tok inspect ./prompt.txt --security-report
```

### Phase 4 — Policy and Blocking

Implement:

- Policy evaluator
- Observe/balanced/strict/developer modes
- Block behavior
- Redacted reports

Deliverable:

```bash
tok run ./prompt.txt --security
```

blocks private keys and passwords safely.

### Phase 5 — Obfuscation and Restoration

Implement:

- Placeholder obfuscation
- Memory-only map
- Response restoration
- Optimizer placeholder preservation

Deliverable:

Provider receives sanitized prompt, user receives restored response.

### Phase 6 — Proxy Security

Implement security in proxy mode.

Deliverable:

```bash
tok proxy start --security
```

Works with OpenAI-compatible clients.

### Phase 7 — llama.cpp Runtime

Implement:

- Runtime manager
- Binary resolver
- Model resolver
- Health checks
- JSON completion client
- `tok doctor --slm`
- `tok models verify`

Deliverable:

```bash
tok run ./prompt.txt --security --slm
```

Uses local SLM for semantic scanning.

### Phase 8 — Docs and Hardening

Implement:

- Security docs
- Runtime docs
- Config docs
- Test coverage
- Redacted logs
- Error messages

---

## 30. Cursor Master Prompt

Use the following prompt in Cursor.

```md
# Add Optional Security Layer to TOK

You are working in the existing TOK repository:

`git@github.com-mantis:MantisWare/tok.git`

TOK is a Token Optimization Kit: a CLI/proxy that reduces LLM token usage while preserving the same external commands and provider interactions.

Your task is to add an optional local-first security/privacy layer to TOK.

## Critical Instruction

Do not rewrite TOK from scratch.

Inspect the current repository structure first and integrate into the existing CLI, proxy, optimizer, provider, config, logging, and test architecture.

The security layer must be config-driven and optional.

Default behavior should preserve existing TOK behavior unless security is explicitly enabled.

## Product Goal

When enabled, TOK security mode should:

1. Scan user input for sensitive data.
2. Detect PII, credentials, secrets, internal hostnames, database URLs, financial values, and other sensitive entities.
3. Block high-risk secrets by default.
4. Obfuscate lower-risk sensitive values with deterministic placeholders.
5. Run the existing TOK optimizer on the sanitized prompt.
6. Send only sanitized content to the LLM provider.
7. Intercept the provider response.
8. Restore obfuscated values locally.
9. Return the final restored response to the user.
10. Produce a redacted security report.

## Config

Add or extend TOK config with:

```yaml
security:
  enabled: false
  mode: balanced

  scan:
    deterministic: true
    slm: false

  actions:
    email: placeholder
    phone: placeholder
    person: placeholder
    company: placeholder
    client: placeholder
    internal_project: placeholder
    url: placeholder
    hostname: placeholder
    ip_address: placeholder
    money: placeholder
    api_key: block
    jwt: block
    private_key: block
    password: block
    database_url: block
    credit_card: block
    bank_account: block

  restore:
    enabled: true
    exact: true
    aliases: false
    fuzzy: false
    validate_with_slm: false

  logging:
    store_original_prompts: false
    store_sanitized_prompts: true
    redact_logs: true

slm:
  enabled: false
  runtime: embedded-llamacpp
  model_path: ./models/tok-security-slm/model.gguf
  context_size: 8192
  temperature: 0.1
  max_tokens: 1200
  startup_timeout_ms: 30000
  bind_host: 127.0.0.1
```

## CLI Flags

Add flags where appropriate:

```bash
--security
--no-security
--security-mode observe|balanced|strict|developer
--security-report
--slm
--no-slm
```

Add or extend commands:

```bash
tok inspect <file> --security-report
tok doctor --slm
tok models verify
```

## Security Pipeline

Integrate this flow around the existing TOK optimizer:

```text
input prompt
  -> security scanner
  -> policy evaluator
  -> block if needed
  -> obfuscation engine
  -> existing TOK optimizer
  -> provider adapter
  -> response restoration
  -> final response
```

Security scanning should happen before optimization.

Optimization should run on sanitized text.

Provider adapters must never receive original sensitive values when security is enabled.

## Deterministic Scanners

Implement scanners for:

- email
- phone
- URL
- hostname
- IP address
- API key
- JWT
- private key
- password
- database URL
- credit card-like values with Luhn validation
- bank account-like values
- money values

High-risk entities should block by default:

- private keys
- passwords
- database URLs
- JWTs
- API keys
- credit cards
- bank accounts

Do not print detected secret values in output or logs.

## Obfuscation

Implement deterministic placeholder replacement.

Use placeholder format:

```text
{{TOK_EMAIL_001}}
{{TOK_PERSON_001}}
{{TOK_COMPANY_001}}
{{TOK_URL_001}}
{{TOK_SECRET_001}}
```

Store original values in a memory-only obfuscation map.

The map must never be sent to external providers.

## Restoration

After provider response, restore placeholders using the local obfuscation map.

Report unresolved placeholders.

Do not enable fuzzy restoration in MVP unless config explicitly enables it.

## Optimizer Integration

Ensure the existing TOK optimizer preserves TOK placeholders.

Add tests to prove placeholders survive optimization.

## SLM and llama.cpp

Add optional SLM support using embedded llama.cpp.

The SLM is optional and disabled by default.

When enabled, it should assist with semantic detection and validation.

Implement runtime support for:

- platform-specific llama.cpp `llama-server` binary resolution
- user-provided or bundled `.gguf` model path
- starting server as child process
- binding only to `127.0.0.1`
- random available port
- health check
- JSON-only completion
- parsing and repairing JSON
- clean shutdown

Use this interface:

```ts
export interface LocalSlmRuntime {
  start(): Promise<void>;
  stop(): Promise<void>;
  isRunning(): boolean;
  healthCheck(): Promise<boolean>;
  completeJson<T>(request: SlmJsonRequest): Promise<T>;
}

export interface SlmJsonRequest {
  systemPrompt: string;
  userPrompt: string;
  schemaName: string;
  maxTokens?: number;
  temperature?: number;
}
```

Recommended default custom SLM base:

```text
Qwen3-4B-Instruct GGUF Q4_K_M
```

Alternative:

```text
Phi-4-mini-instruct GGUF Q4_K_M
```

Model path:

```text
models/tok-security-slm/model.gguf
```

## SLM Scanner Prompt

Use JSON-only output.

The SLM should return:

```json
{
  "sensitive_entities": [
    {
      "type": "person",
      "value": "exact text span",
      "confidence": 0.8,
      "recommended_action": "placeholder",
      "reason": "short reason"
    }
  ],
  "risk_level": "low",
  "safe_to_send": true,
  "warnings": []
}
```

Deterministic scanner findings override SLM findings.

Secret scanner findings override everything.

## Logging Rules

Default logs may include:

- request id
- timestamp
- security enabled
- mode
- risk level
- entity counts
- action counts
- provider
- model
- token savings

Default logs must never include:

- original prompt
- original sensitive values
- private keys
- passwords
- API keys
- database URLs
- obfuscation map
- restored response

## Tests

Add tests for:

- all deterministic detectors
- policy modes
- blocking high-risk secrets
- obfuscation
- restoration
- optimizer placeholder preservation
- provider adapter receives sanitized prompt only
- proxy security flow
- redacted logging
- llama.cpp runtime manager with mocked child process

## Implementation Order

1. Inspect repo and identify integration points.
2. Add config schema and CLI flags.
3. Add security types.
4. Add deterministic scanners.
5. Add policy evaluator.
6. Add placeholder obfuscation.
7. Add response restoration.
8. Integrate into CLI flow.
9. Integrate into proxy flow.
10. Add tests proving provider does not receive originals.
11. Add llama.cpp runtime package/module.
12. Add optional SLM scanner.
13. Add doctor/model verification commands.
14. Update docs and README.

Keep the implementation incremental, typed, tested, and compatible with existing TOK behavior.
```

---

## 31. Important Non-Goals for MVP

Do not implement these in the first pass unless the repo already has strong foundations for them:

```text
team policy server
cloud dashboard
browser extension
desktop app
encrypted vault
fake data generation
fuzzy restoration
real streaming restoration
remote model downloads
fine-tuning workflows
```

Keep MVP focused:

```text
scan
block
obfuscate
optimize
send
restore
report
optional SLM
```

---

## 32. Final Acceptance Criteria

The implementation is acceptable when:

1. TOK works exactly as before when security is disabled.
2. TOK can run with `--security`.
3. TOK blocks private keys, passwords, JWTs, database URLs, and obvious API keys by default.
4. TOK obfuscates emails, URLs, phones, money, hostnames, and similar values.
5. Provider adapters receive sanitized content only.
6. Responses are restored locally.
7. Logs do not contain original sensitive values.
8. Tests prove secrets are not leaked.
9. SLM support can be enabled optionally.
10. llama.cpp runtime can be verified with `tok doctor --slm`.
11. The README documents security mode clearly.
12. Proxy mode supports security with clear streaming behavior.

---

## 33. Suggested README Addition

```md
## Optional Security Mode

TOK can optionally scan and sanitize prompts before they are optimized and sent to an LLM provider.

Enable it with:

```bash
tok run prompt.txt --security
```

or in proxy mode:

```bash
tok proxy start --security
```

Security mode can:

- Detect PII
- Detect secrets
- Block high-risk credentials
- Replace sensitive values with placeholders
- Optimize the sanitized prompt
- Restore placeholders in the model response

By default, TOK does not log original prompts or sensitive values.

### Optional Local SLM

TOK can use a local SLM through embedded llama.cpp for semantic detection:

```bash
tok run prompt.txt --security --slm
```

Configure the model path:

```yaml
slm:
  enabled: true
  runtime: embedded-llamacpp
  model_path: ./models/tok-security-slm/model.gguf
```

Recommended model:

```text
Qwen3-4B-Instruct GGUF Q4_K_M
```
```

