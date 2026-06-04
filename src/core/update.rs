//! Self-update: check GitHub releases and upgrade the installed binary.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::constants::TOK_DATA_DIR;
use crate::core::utils;

const REPO: &str = "MantisWare/tok";
const GITHUB_API: &str = "https://api.github.com/repos/MantisWare/tok/releases/latest";
const CHECK_INTERVAL_SECS: u64 = 24 * 3600;
const WARN_INTERVAL_SECS: u64 = 24 * 3600;
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Outdated { latest: String, latest_tag: String },
    Unknown,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct UpdateCache {
    latest: String,
    latest_tag: String,
    checked_at: i64,
}

/// Current version baked in at compile time (without leading `v`).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Rate-limited stderr warning when a newer release is available.
pub fn maybe_warn() {
    let _ = warn_if_outdated();
}

fn warn_if_outdated() -> Result<()> {
    if std::env::var("TOK_UPDATE_CHECK").ok().as_deref() == Some("0") {
        return Ok(());
    }

    let status = check_cached(false)?;
    let UpdateStatus::Outdated { latest, .. } = status else {
        return Ok(());
    };

    let marker = warn_marker_path();
    if let Some(path) = marker.as_ref() {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().unwrap_or(Duration::MAX).as_secs() < WARN_INTERVAL_SECS {
                    return Ok(());
                }
            }
        }
    }

    eprintln!(
        "[tok] Update available: v{latest} (current: v{}) — run `tok update`",
        current_version()
    );

    if let Some(path) = marker {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, b"");
    }

    Ok(())
}

/// Check for updates, optionally refreshing the cache from GitHub.
pub fn check_cached(refresh_if_stale: bool) -> Result<UpdateStatus> {
    if std::env::var("TOK_UPDATE_CHECK").ok().as_deref() == Some("0") {
        return Ok(UpdateStatus::Unknown);
    }

    let cache_path = cache_file_path();
    let cached = cache_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<UpdateCache>(&s).ok());

    let cache_fresh = cached.as_ref().is_some_and(|c| {
        chrono::Utc::now().timestamp() - c.checked_at < CHECK_INTERVAL_SECS as i64
    });

    if cache_fresh {
        return Ok(status_from_cache(cached.as_ref()));
    }

    if refresh_if_stale {
        if let Ok(release) = fetch_latest_release() {
            write_cache(&release)?;
            return Ok(compare_with_current(&release.version, &release.tag));
        }
    }

    if let Some(cache) = cached {
        return Ok(status_from_cache(Some(&cache)));
    }

    Ok(UpdateStatus::Unknown)
}

fn status_from_cache(cache: Option<&UpdateCache>) -> UpdateStatus {
    let Some(cache) = cache else {
        return UpdateStatus::Unknown;
    };
    compare_with_current(&cache.latest, &cache.latest_tag)
}

fn compare_with_current(latest: &str, latest_tag: &str) -> UpdateStatus {
    if version_cmp(latest, current_version()) == std::cmp::Ordering::Greater {
        UpdateStatus::Outdated {
            latest: latest.to_string(),
            latest_tag: latest_tag.to_string(),
        }
    } else {
        UpdateStatus::UpToDate
    }
}

struct ReleaseInfo {
    version: String,
    tag: String,
}

fn fetch_latest_release() -> Result<ReleaseInfo> {
    let response = ureq::get(GITHUB_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "tok-self-update")
        .timeout(HTTP_TIMEOUT)
        .call()
        .context("failed to reach GitHub releases API")?;

    let release: GhRelease = serde_json::from_str(
        &response
            .into_string()
            .context("failed to read GitHub release response")?,
    )
    .context("failed to parse GitHub release JSON")?;

    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    Ok(ReleaseInfo {
        version: version.to_string(),
        tag: release.tag_name,
    })
}

fn write_cache(release: &ReleaseInfo) -> Result<()> {
    let Some(path) = cache_file_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create update cache dir")?;
    }
    let cache = UpdateCache {
        latest: release.version.clone(),
        latest_tag: release.tag.clone(),
        checked_at: chrono::Utc::now().timestamp(),
    };
    std::fs::write(&path, serde_json::to_string(&cache)?).context("write update cache")?;
    Ok(())
}

/// Run `tok update` — check only or install the latest release.
pub fn run(check_only: bool) -> Result<()> {
    let release = fetch_latest_release().context(
        "could not fetch latest release — check your network connection or try again later",
    )?;

    write_cache(&release)?;

    let current = current_version();
    match compare_with_current(&release.version, &release.tag) {
        UpdateStatus::UpToDate => {
            println!("tok v{current} is already the latest version");
            return Ok(());
        }
        UpdateStatus::Outdated { latest, latest_tag } => {
            println!("Update available: v{current} → v{latest} ({latest_tag})");
            if check_only {
                println!("Run `tok update` to install the latest version");
                return Ok(());
            }
        }
        UpdateStatus::Unknown => {}
    }

    if check_only {
        println!("Run `tok update` to install the latest version");
        return Ok(());
    }

    perform_update(&release)
}

fn perform_update(release: &ReleaseInfo) -> Result<()> {
    let method = detect_install_method();
    match method {
        "homebrew" => update_via_homebrew(),
        "cargo" => update_via_cargo(),
        "nix" => {
            println!("tok was installed via Nix — update through your Nix channel or flake");
            Ok(())
        }
        _ => update_via_github_release(release),
    }
}

fn update_via_homebrew() -> Result<()> {
    println!("Updating via Homebrew...");
    let status = utils::resolved_command("brew")
        .args(["upgrade", "tok"])
        .status()
        .context("failed to run brew upgrade tok")?;
    if status.success() {
        println!("Update complete — run `tok --version` to verify");
    } else {
        bail!(
            "brew upgrade tok failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn update_via_cargo() -> Result<()> {
    println!("Updating via cargo install...");
    let status = utils::resolved_command("cargo")
        .args([
            "install",
            "tok",
            "--git",
            "https://github.com/MantisWare/tok",
            "--force",
        ])
        .status()
        .context("failed to run cargo install")?;
    if status.success() {
        println!("Update complete — run `tok --version` to verify");
    } else {
        bail!(
            "cargo install failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn update_via_github_release(release: &ReleaseInfo) -> Result<()> {
    let target = target_triple()?;
    let (url, archive_name, binary_name) = release_asset(&release.tag, &target)?;

    println!("Downloading {url}...");
    let temp_dir = tempfile::tempdir().context("create temp dir for update")?;
    let archive_path = temp_dir.path().join(&archive_name);

    let response = ureq::get(&url)
        .timeout(Duration::from_secs(60))
        .call()
        .context("failed to download release archive")?;
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(&archive_path).context("create archive file")?;
    std::io::copy(&mut reader, &mut file).context("failed to write release archive")?;

    let extract_dir = temp_dir.path().join("extract");
    std::fs::create_dir_all(&extract_dir).context("create extract dir")?;
    extract_archive(&archive_path, &extract_dir, &target)?;

    let new_binary = extract_dir.join(binary_name);
    if !new_binary.exists() {
        bail!("extracted archive did not contain expected binary `{binary_name}`");
    }

    let current_exe = std::env::current_exe().context("resolve current tok binary path")?;
    replace_binary(&new_binary, &current_exe)?;

    println!("Update complete — tok v{}", release.version);
    Ok(())
}

fn release_asset(tag: &str, target: &str) -> Result<(String, String, &'static str)> {
    if target.contains("windows") {
        Ok((
            format!("https://github.com/{REPO}/releases/download/{tag}/tok-{target}.zip"),
            format!("tok-{target}.zip"),
            "tok.exe",
        ))
    } else {
        Ok((
            format!("https://github.com/{REPO}/releases/download/{tag}/tok-{target}.tar.gz"),
            format!("tok-{target}.tar.gz"),
            "tok",
        ))
    }
}

fn extract_archive(archive: &Path, dest: &Path, target: &str) -> Result<()> {
    if target.contains("windows") {
        extract_zip(archive, dest)
    } else {
        let status = utils::resolved_command("tar")
            .args([
                "-xzf",
                archive.to_str().context("archive path not UTF-8")?,
                "-C",
                dest.to_str().context("dest path not UTF-8")?,
            ])
            .status()
            .context("failed to run tar")?;
        if !status.success() {
            bail!("tar extraction failed");
        }
        Ok(())
    }
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let status = utils::resolved_command("tar")
        .args([
            "-xf",
            archive.to_str().context("archive path not UTF-8")?,
            "-C",
            dest.to_str().context("dest path not UTF-8")?,
        ])
        .status()
        .context("failed to run tar for zip extraction")?;
    if status.success() {
        return Ok(());
    }

    let archive_str = archive.to_str().context("archive path not UTF-8")?;
    let dest_str = dest.to_str().context("dest path not UTF-8")?;
    let ps_script =
        format!("Expand-Archive -Force -Path '{archive_str}' -DestinationPath '{dest_str}'");
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .status()
        .context("failed to run PowerShell Expand-Archive")?;
    if status.success() {
        Ok(())
    } else {
        bail!("failed to extract zip archive");
    }
}

fn replace_binary(new_binary: &Path, current_exe: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::copy(new_binary, current_exe).context("replace tok binary")?;
        std::fs::set_permissions(current_exe, std::fs::Permissions::from_mode(0o755))
            .context("set executable permissions")?;
        Ok(())
    }

    #[cfg(windows)]
    {
        let backup = current_exe.with_extension("old.exe");
        let _ = std::fs::remove_file(&backup);
        if current_exe.exists() {
            std::fs::rename(current_exe, &backup).context("move current binary aside")?;
        }
        std::fs::copy(new_binary, current_exe).context("install new tok binary")?;
        let _ = std::fs::remove_file(&backup);
        Ok(())
    }
}

fn target_triple() -> Result<String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl".to_string()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu".to_string()),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc".to_string()),
        (os, arch) => bail!("unsupported platform for self-update: {os} {arch}"),
    }
}

pub(crate) fn detect_install_method() -> &'static str {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return "unknown",
    };
    let real_path = std::fs::canonicalize(&exe)
        .unwrap_or(exe)
        .to_string_lossy()
        .to_string();
    install_method_from_path(&real_path)
}

pub(crate) fn install_method_from_path(path: &str) -> &'static str {
    if path.contains("/Cellar/tok/") || path.contains("/homebrew/") {
        "homebrew"
    } else if path.contains("/.cargo/bin/") || path.contains("\\.cargo\\bin\\") {
        "cargo"
    } else if path.contains("/.local/bin/") || path.contains("\\.local\\bin\\") {
        "script"
    } else if path.contains("/nix/store/") {
        "nix"
    } else {
        "other"
    }
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let va = parse_version(a);
    let vb = parse_version(b);
    for i in 0..va.len().max(vb.len()) {
        let ai = va.get(i).copied().unwrap_or(0);
        let bi = vb.get(i).copied().unwrap_or(0);
        match ai.cmp(&bi) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn parse_version(raw: &str) -> Vec<u32> {
    let core = raw
        .strip_prefix('v')
        .unwrap_or(raw)
        .split('-')
        .next()
        .unwrap_or(raw);
    core.split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn cache_file_path() -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join(TOK_DATA_DIR)
            .join(".update_check_cache"),
    )
}

fn warn_marker_path() -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join(TOK_DATA_DIR)
            .join(".update_warn_last"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_strips_v_prefix() {
        assert_eq!(parse_version("v1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version("0.1.21"), vec![0, 1, 21]);
    }

    #[test]
    fn test_parse_version_prerelease_suffix() {
        assert_eq!(parse_version("1.0.0-beta.1"), vec![1, 0, 0]);
    }

    #[test]
    fn test_version_cmp() {
        assert_eq!(version_cmp("0.1.22", "0.1.21"), std::cmp::Ordering::Greater);
        assert_eq!(version_cmp("0.1.21", "0.1.21"), std::cmp::Ordering::Equal);
        assert_eq!(version_cmp("0.1.20", "0.1.21"), std::cmp::Ordering::Less);
        assert_eq!(version_cmp("1.0.0", "0.9.9"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_compare_with_current() {
        let current = current_version();
        assert_eq!(
            compare_with_current(current, &format!("v{current}")),
            UpdateStatus::UpToDate
        );
        assert_eq!(
            compare_with_current("99.0.0", "v99.0.0"),
            UpdateStatus::Outdated {
                latest: "99.0.0".to_string(),
                latest_tag: "v99.0.0".to_string(),
            }
        );
    }

    #[test]
    fn test_install_method_paths() {
        assert_eq!(
            install_method_from_path("/opt/homebrew/Cellar/tok/0.1.0/bin/tok"),
            "homebrew"
        );
        assert_eq!(
            install_method_from_path("/home/user/.cargo/bin/tok"),
            "cargo"
        );
        assert_eq!(
            install_method_from_path("/home/user/.local/bin/tok"),
            "script"
        );
        assert_eq!(
            install_method_from_path("/nix/store/abc123-tok/bin/tok"),
            "nix"
        );
    }

    #[test]
    fn test_target_triple_known_platforms() {
        let triple = target_triple().expect("host should be supported");
        assert!(triple.contains('-'));
    }

    #[test]
    fn test_release_asset_names() {
        let (url, archive, binary) =
            release_asset("v0.1.21", "x86_64-apple-darwin").expect("asset");
        assert!(url.contains("tok-x86_64-apple-darwin.tar.gz"));
        assert_eq!(archive, "tok-x86_64-apple-darwin.tar.gz");
        assert_eq!(binary, "tok");

        let (url, archive, binary) =
            release_asset("v0.1.21", "x86_64-pc-windows-msvc").expect("asset");
        assert!(url.contains(".zip"));
        assert_eq!(archive, "tok-x86_64-pc-windows-msvc.zip");
        assert_eq!(binary, "tok.exe");
    }
}
