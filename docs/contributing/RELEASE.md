# Release process (maintainers)

This document describes how **stable releases**, **GitHub Release assets**, and the **[Homebrew tap](https://github.com/MantisWare/homebrew-tap)** stay in sync. For day-to-day development, see [DEVELOPMENT.md](DEVELOPMENT.md). For CI diagrams, see [.github/workflows/CICD.md](../../.github/workflows/CICD.md).

## What gets published

On each **stable** release (not pre-releases), CI:

1. Builds binaries for multiple targets (including **macOS** Intel and Apple Silicon, Linux, Windows).
2. Builds **DEB** and **RPM** packages.
3. Uploads everything to a **[GitHub Release](https://github.com/MantisWare/tok/releases)** on this repo (archives, packages, `checksums.txt`).
4. Optionally notifies **Discord** (if `TOK_DISCORD_RELEASE` is set).
5. Updates **`Formula/tok.rb`** in **[MantisWare/homebrew-tap](https://github.com/MantisWare/homebrew-tap)** (if `HOMEBREW_TAP_TOKEN` is set).
6. Moves the **`latest`** git tag to the new stable version.

**Pre-releases** (e.g. `dev-*` tags from the `develop` branch) upload assets but **do not** update Homebrew or post to Discord.

---

## One-time setup (before your first Homebrew-automated release)

Complete these once per organization / fork.

### 1. Homebrew tap repository

- Repo: **[github.com/MantisWare/homebrew-tap](https://github.com/MantisWare/homebrew-tap)**  
- Must contain a `Formula/` directory (the workflow writes `Formula/tok.rb` via the GitHub API).

### 2. Secret on `MantisWare/tok`: `HOMEBREW_TAP_TOKEN`

The workflow pushes the updated formula using a token that can write to **`MantisWare/homebrew-tap`**.

1. Create a **fine-grained personal access token** (or classic PAT with `repo` scope) with **Contents: Read and write** on **`MantisWare/homebrew-tap` only** (least privilege).
2. In **`MantisWare/tok`**: **Settings → Secrets and variables → Actions → New repository secret**.
3. **Name:** `HOMEBREW_TAP_TOKEN` (exact spelling).
4. **Value:** paste the token.

Without this secret, the release still succeeds, but the **Update Homebrew formula** job fails or is skipped; users can still install via **Cargo** or release assets.

### 3. Optional secrets and variables

| Name | Purpose |
|------|--------|
| `TOK_DISCORD_RELEASE` | Webhook URL for release announcements (stable releases only). |
| `TOK_TELEMETRY_URL` / `TOK_TELEMETRY_TOKEN` | Optional telemetry endpoints used during **release** builds (see `.github/workflows/release.yml`). |

---

## Normal path: release-please on `master`

Releases are driven by **[release-please](https://github.com/googleapis/release-please)** and **[Conventional Commits](https://www.conventionalcommits.org/)** on the default release branch (`master` in this repo).

### Day-to-day

1. Land changes on `develop` (or your integration branch) using conventional prefixes, for example:
   - `feat:` — minor bump (0.x **minor** while on 0.x per config)
   - `fix:` — patch bump
   - `feat!:` / `BREAKING CHANGE:` — major bump after 1.0
2. Merge **`develop` → `master`** when you want a release train (via PR or merge policy your team uses).

### What release-please does

On **push to `master`**, the **CD** workflow runs **release-please**:

- It opens or updates a **Release PR** that bumps **version** (e.g. in `Cargo.toml`), updates **CHANGELOG.md**, and similar.
- **Merge that Release PR** when you are ready to ship that version.

After the Release PR is merged, release-please can **create the GitHub Release and tag** in a subsequent run. When **`release_created`** is true, the workflow invokes **`release.yml`**, which builds artifacts and runs the steps in the first section above.

### After the release

- Confirm **[Releases](https://github.com/MantisWare/tok/releases)** lists the new tag and assets.
- Confirm **[homebrew-tap/Formula/tok.rb](https://github.com/MantisWare/homebrew-tap/blob/main/Formula/tok.rb)** matches the new version and SHA256 lines (if `HOMEBREW_TAP_TOKEN` is configured).
- Smoke-test on macOS:
  ```bash
  brew update
  brew upgrade tok
  # or fresh install:
  brew tap MantisWare/tap
  brew install tok
  tok --version
  ```

---

## Manual release (workflow dispatch)

Use this for emergencies or when you need to cut a release outside the usual release-please PR flow.

1. Go to **Actions** → workflow **Release** (`.github/workflows/release.yml`).
2. **Run workflow**.
3. **tag:** e.g. `v0.1.0` (must match an existing tag **or** be created consistently with how `release.yml` expects inputs—prefer aligning with release-please versioning).
4. **prerelease:** unchecked for a **stable** release (so Discord + Homebrew + `latest` tag run).

**Note:** Manually creating a GitHub Release from the UI without running this workflow **does not** build or attach the same artifacts; prefer the workflow so binaries and the tap stay consistent.

---

## Branch reference

| Branch / event | Typical outcome |
|----------------|-----------------|
| Push to **`develop`** | Pre-release tag (`dev-…-rc.…`), assets marked pre-release; **no** Homebrew update. |
| Push to **`master`** | release-please; merge Release PR → stable tag → full pipeline including Homebrew (if configured). |

---

## Troubleshooting

| Symptom | Things to check |
|---------|-------------------|
| Homebrew formula not updated | Stable release? (`prerelease` must be false.) Secret `HOMEBREW_TAP_TOKEN` present and authorized for `MantisWare/homebrew-tap`? Job **Update Homebrew formula** logs in Actions. |
| No GitHub Release assets | **Release** job failed? Check **build** matrix and artifact upload steps. |
| release-please did not open a PR | Conventional commits on `master`? Config in `release-please-config.json` / manifest. |
| Wrong `tok` after `brew install` | Run `which tok` and `tok gain`; see [INSTALL.md](../../INSTALL.md) and [TROUBLESHOOTING.md](../TROUBLESHOOTING.md). |

---

## Related files

- `.github/workflows/cd.yml` — release-please, calls `release.yml` on stable releases.
- `.github/workflows/release.yml` — builds, GitHub Release, Discord, Homebrew tap update.
- `release-please-config.json` / `.release-please-manifest.json` — versioning for release-please.
- `Formula/tok.rb` (this repo) — template; the **published** formula lives in **homebrew-tap**.
