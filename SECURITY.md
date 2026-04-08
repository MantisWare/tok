# Security Policy

## Reporting a vulnerability

Found something sketchy? Please tell us privately before it turns into a Twitter thread:

- **Email**: security@tok-ai.dev
- **GitHub**: [Private security advisory](https://github.com/tok-ai/tok/security/advisories/new) (preferred when available)
- **Response time**: We aim to acknowledge reports within 48 hours
- **Disclosure**: We follow responsible disclosure practices (90-day embargo by default)

**Please do NOT:**

- Open public GitHub issues for security vulnerabilities
- Disclose vulnerabilities on social media or forums before we've had a chance to address them

---

## Threat model (short)

TOK is a **command-line proxy**: it runs the same underlying tools you would run directly (git, cargo, npm, shells where used, etc.). Security expectations:

- **Trusted user on their own machine**: TOK does not sandbox children; a malicious filter bug could alter arguments or output handling in surprising ways, but the primary risk remains **whatever commands you already chose to run**.
- **Hooks** (e.g. IDE integrations under `hooks/`) **rewrite or wrap commands** before execution. Treat hook scripts like any other executable config: only install hooks from sources you trust, and review updates.
- **Local data**: Command history and metrics are stored locally (see [Privacy & local data](#privacy--local-data)); backups and shared machines increase exposure of that data.

---

## Security Review Process for Pull Requests

TOK executes subprocesses and handles user-controlled input. PRs from external contributors undergo enhanced security review to protect against:

- **Shell injection** (command construction and argument passing)
- **Supply chain attacks** (malicious or typosquatted dependencies)
- **Backdoors** (logic bombs, covert exfiltration)
- **Data leaks** (unintended logging, overly broad telemetry, or unsafe handling of history databases)

---

## Automated Security Checks

Pull requests against `develop` / `master` run the **Security Scan** job in [`.github/workflows/ci.yml`](.github/workflows/ci.yml) (`security` job). It includes:

1. **Dependency audit** (`cargo audit`) — known Rust ecosystem CVEs (warnings are surfaced in the job summary; maintainers judge severity)
2. **Critical files alert** — lists changes touching high-risk basenames (see regex in the workflow: `runner`, `summary`, `tracking`, `init`, `pnpm_cmd`, `container`, plus `Cargo.toml` and workflow YAML)
3. **Dangerous pattern scan** (diff-based) — flags additions such as:
   - `Command::new("sh")` / `Command::new("bash")`
   - Sensitive `.env(...)` usage (e.g. `LD_PRELOAD`, `PATH`)
   - Network primitives (`reqwest::`, `std::net::`, `TcpStream`, `UdpSocket`) — note: allowed HTTP client crates may evolve; treat any new outbound network code as high scrutiny
   - `unsafe { ... }`
   - `.unwrap()`, `panic!`, `todo!`, `unimplemented!` in changed lines
4. **New dependencies** — diff of `Cargo.toml` to prompt manual supply-chain review
5. **Clippy** — extra warnings for `unwrap_used`, `panic`, `expect_used` in that step

Results are posted in the PR's GitHub Actions **job summary** for the Security Scan job.

---

## Critical Files Requiring Enhanced Review

The following paths are **high-risk** and should trigger careful human review (and align with the automated critical-file basename checks in CI):

### Tier 1: Execution, aggregation, and rewrites

- **`src/core/runner.rs`** — Core command execution / routing
- **`src/cmds/system/summary.rs`** — Command output aggregation
- **`src/core/tracking.rs`** — SQLite history / metrics
- **`src/discover/registry.rs`** — Discovery and rewrite registration (broad impact if wrong)
- **`src/hooks/init.rs`**, **`src/hooks/rewrite_cmd.rs`**, **`src/hooks/trust.rs`**, **`src/hooks/integrity.rs`** — Hook installation, rewrite, and trust surfaces
- **Hook scripts** (examples: `hooks/**/tok-rewrite.sh`, `.claude/hooks/tok-rewrite.sh`) — Run inside editor/agent contexts and can change how commands are invoked

### Tier 2: Input validation and elevated operations

- **`src/cmds/js/pnpm_cmd.rs`** — Package / CLI argument surfaces
- **`src/cmds/cloud/container.rs`** — Container-related invocation

### Tier 3: Supply chain and CI/CD

- **`Cargo.toml`** / **`Cargo.lock`** — Dependencies
- **`.github/workflows/*.yml`** — Build, release, and automation integrity

**If your PR modifies any of these**, expect:

- Detailed manual security review
- Questions about design and threat assumptions
- A potentially slower merge timeline

---

## Review Workflow

### For external contributors

1. **Submit PR** → Security Scan job runs on CI
2. **Read the job summary** → Resolve or explain anything flagged
3. **Manual review** → Maintainer validates intent vs. diff
4. **Approval** → Merge (or request changes)

### For maintainers

```bash
gh pr view <PR_NUMBER>
gh pr diff <PR_NUMBER> > /tmp/pr.diff

# Mirror the CI "dangerous patterns" scan locally (adjust base ref if needed):
git fetch origin master
grep -E 'Command::new\("sh"|Command::new\("bash"|\.env\("LD_PRELOAD|\.env\("PATH|reqwest::|std::net::|TcpStream|UdpSocket|unsafe \{|\.unwrap\(\) |panic!\(|todo!\(|unimplemented!\(' /tmp/pr.diff || true
```

**Review checklist:**

- [ ] No critical files modified **or** changes justified and reviewed by two maintainers when policy requires it
- [ ] No dangerous patterns **or** each hit explained and proven safe
- [ ] No new dependencies **or** each dependency vetted (crates.io reputation, downloads, license, recent activity)
- [ ] PR description matches the actual code changes
- [ ] No logic bombs (time-based triggers, unexplained environment branching to “call home”)
- [ ] No unexplained complexity spikes or opaque encoding (e.g. large base64 blobs without justification)

---

## Privacy & local data

### Command history database

TOK records usage metrics locally for features like `tok gain` and history. Default database file: **`history.db`** under the platform data directory (e.g. `~/.local/share/tok/` on Linux, `~/Library/Application Support/tok/` on macOS). Override order:

1. Environment variable **`TOK_DB_PATH`**
2. `config.toml` → `tracking.database_path`
3. Default path above

Stored data can include **command strings and paths** from your workflow. Treat the database like sensitive local state on **shared computers, backups, and bug report attachments**. Retention is configurable; see [docs/usage/TRACKING.md](docs/usage/TRACKING.md).

### Optional telemetry

If the binary is built **with** a compile-time telemetry endpoint (`TOK_TELEMETRY_URL`), the CLI may send **non-blocking**, **aggregated** usage statistics (e.g. version, OS, rough install method, token savings aggregates, top command names). There is **no telemetry** in builds without that URL compiled in.

**Opt out:**

- Environment: `TOK_TELEMETRY_DISABLED=1`
- Config: `[telemetry] enabled = false` in `config.toml` (see `src/core/config.rs` / user docs)

A per-machine salt file **`.device_salt`** (under the app data directory) participates in a **hashed device identifier**; it is not intended to transmit raw hostnames or usernames in the clear. On Unix, the salt file is created with restrictive permissions where supported.

---

## Dangerous Patterns We Check For

| Pattern | Risk | Example |
|---------|------|---------|
| `Command::new("sh")` / `bash` | Shell injection | User-controlled string interpreted by a shell |
| `.env("LD_PRELOAD")` | Library hijacking | Preloading untrusted shared libraries |
| `.env("PATH")` | Binary hijacking | Wrong executable resolved |
| `reqwest::`, `ureq::`, `std::net::`, sockets | Unexpected egress | Covert or undisclosed network behavior |
| `unsafe {` | Memory safety | Bypasses Rust guarantees |
| `.unwrap()` / `panic!` in production paths | Denial of service | Process abort on unexpected input |
| `SystemTime::now()` in security-sensitive conditionals | Logic bombs | Time-gated behavior |
| Long base64 / hex blobs | Obfuscation | Hides payloads or URLs |

The table above is authoritative for this repo; external wikis may lag.

---

## Dependency Security

New dependencies added to `Cargo.toml` should meet these criteria:

- **Downloads**: Meaningful crates.io usage (or a strong justification if niche)
- **Maintainer**: Identifiable maintainer and history
- **License**: Compatible with the project (e.g. MIT / Apache-2.0 style stack)
- **Activity**: Maintained within a reasonable window for security fixes
- **No typosquatting**: Manually compare against well-known crate names

**Red flags:**

- Brand-new crate with almost no users and no track record
- Maintainer with no reachable identity
- Name easily confused with a popular crate (e.g. `serid` vs `serde`)
- Sudden license or ownership change

---

## Security Best Practices for Contributors

### Avoid these anti-patterns

**Do not:**

```rust
// Shell injection risk
let user_input = get_arg();
Command::new("sh").arg("-c").arg(format!("echo {}", user_input)).output();

// Panic on invalid input
let path = std::env::args().nth(1).unwrap();

// Hardcoded secrets
const API_KEY: &str = "sk_live_1234567890abcdef";
```

**Do:**

```rust
// Prefer no shell; pass argv to a known binary
let user_input = get_arg();
Command::new("echo").arg(user_input).output();

// Propagate errors with context
let path = std::env::args().nth(1).context("Missing path argument")?;

// Secrets from environment or secure storage — not in source
let api_key = std::env::var("API_KEY").context("API_KEY not set")?;
```

### Error handling

- Use `anyhow::Result` with `.context(...)` for operational errors
- Avoid `.unwrap()` in production `src/` code (tests may use it freely)
- Prefer proper error propagation over `.expect(...)` except for rare invariants with a **specific** message

### Input validation

- Validate user-controlled input before it reaches `Command` or path APIs
- Prefer allowlists for flags and subcommands where feasible
- Normalize or canonicalize paths when mixing user input with filesystem operations
- Keep package-name and similar validators strict and well-tested

---

## Disclosure Timeline

When vulnerabilities are reported:

1. **Day 0**: Acknowledgment to the reporter
2. **Day 7**: Severity and impact assessment
3. **Day 14**: Patch development begins (sooner for critical issues)
4. **Day 30**: Target for patch release + CVE (if applicable)
5. **Day 90**: Public disclosure (or earlier coordinated release)

**Critical** issues (e.g. remote code execution, undisclosed data exfiltration) may be fast-tracked.

---

## Security Tooling

| Tool | Role |
|------|------|
| **`cargo audit`** | Runs in CI Security Scan; use locally after dependency changes |
| **`cargo clippy --all-targets`** | Runs in CI; catches many foot-guns |
| **`cargo deny`** | Optional local/org policy (license bans); not required by default CI here |
| **GitHub Dependabot** | Enable per-org/repo policy for automated update PRs |
| **CodeQL / advanced SAST** | Optional hardening; not a merge gate in this repo unless enabled later |

---

## Contact

- **Security issues**: security@tok-ai.dev
- **General questions**: https://github.com/tok-ai/tok/discussions
- **Maintainers**: @FlorianBruniaux (active fork maintainer)

---

**Last updated**: 2026-04-08
