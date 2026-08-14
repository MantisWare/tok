//! Registers the TOK MCP server with each agent host, and injects the graph
//! instruction section into their instruction files.
//!
//! Eight hosts, five config shapes, and no agreement between any of them: some
//! want `mcpServers`, VS Code wants `servers` with an explicit transport,
//! OpenCode nests under `mcp.servers` and takes the command as one array, and
//! Codex uses TOML. The shape differences are the reason this is a table rather
//! than a loop.
//!
//! Two rules hold for every host:
//!
//! - **Merge, never rewrite.** These files hold the user's other servers and
//!   their editor settings. Registration adds one key and leaves the rest byte
//!   for byte, which is also what makes re-running `tok init` safe.
//! - **Register an absolute path.** Editors launched from a desktop icon
//!   inherit a minimal `PATH` that frequently lacks `~/.cargo/bin`, so a bare
//!   `tok` resolves interactively and fails under the GUI.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// The key TOK registers itself under, in every host that takes a name.
pub const SERVER_NAME: &str = "tok";

/// Markers bounding the graph section in an instruction file.
const SECTION_START: &str = "<!-- tok-graph -->";
const SECTION_END: &str = "<!-- /tok-graph -->";

/// Guidance written into each host's instruction file.
///
/// Phrased as "instead of", because an agent that merely knows the tools exist
/// still defaults to reading whole files. The token saving comes from the
/// substitution, not the availability.
const GRAPH_INSTRUCTIONS: &str = r#"<!-- tok-graph -->
## Code graph (TOK)

This repository is indexed as a code graph. Use it instead of reading files to
find things — it is faster and costs a fraction of the tokens.

| Instead of | Use |
| --- | --- |
| Reading files to find where something lives | `tok mem ask "<question>"` |
| Reading a whole file to see what it offers | `tok mem skeleton <file>` |
| `grep` / `rg` for a symbol | `tok mem grep <pattern>` |
| Exploring an unfamiliar area | `tok mem map` |

`tok mem ask` returns the symbols worth reading, including callers and callees
that share no words with the query. Read the files it names, not the ones you
guessed. The same six tools are available over MCP as `tok_ask`, `tok_skeleton`,
`tok_grep`, `tok_map`, `tok_relations`, and `tok_check`.

The index refreshes itself when files change, so results are current. It is a
cache: everything under `.tok/graph/` can be deleted and will rebuild.
<!-- /tok-graph -->"#;

/// The agent hosts TOK can wire up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Claude,
    Cursor,
    Codex,
    Gemini,
    Copilot,
    Windsurf,
    Cline,
    OpenCode,
}

/// How a host expects its MCP servers to be spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `{"mcpServers": {"tok": {"command": …, "args": […]}}}` — the de facto
    /// standard, used by Claude, Cursor, Gemini, Windsurf, and Cline.
    McpServers,
    /// VS Code: `{"servers": {"tok": {"type": "stdio", …}}}`.
    VsCode,
    /// OpenCode: `{"mcp": {"servers": {"tok": {"type": "local",
    /// "command": ["tok", "mcp"]}}}}` — one array, not command plus args.
    OpenCode,
    /// Codex: `[mcp_servers.tok]` in TOML.
    Toml,
}

/// Where the files live, kept injectable so tests never touch a real home.
#[derive(Debug, Clone)]
pub struct Layout {
    pub home: PathBuf,
    pub repo_root: PathBuf,
}

impl Layout {
    pub fn detect(repo_root: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            home: dirs::home_dir().context("Could not determine the home directory")?,
            repo_root: repo_root.into(),
        })
    }
}

/// What a registration attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Added,
    /// Already registered with the same command; the file was left alone.
    AlreadyPresent,
    /// Registered, but pointing somewhere else — usually an older install.
    Updated,
}

impl Host {
    pub fn all() -> [Host; 8] {
        [
            Host::Claude,
            Host::Cursor,
            Host::Codex,
            Host::Gemini,
            Host::Copilot,
            Host::Windsurf,
            Host::Cline,
            Host::OpenCode,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Host::Claude => "Claude Code",
            Host::Cursor => "Cursor",
            Host::Codex => "Codex",
            Host::Gemini => "Gemini CLI",
            Host::Copilot => "GitHub Copilot",
            Host::Windsurf => "Windsurf",
            Host::Cline => "Cline",
            Host::OpenCode => "OpenCode",
        }
    }

    fn shape(self) -> Shape {
        match self {
            Host::Claude | Host::Cursor | Host::Gemini | Host::Windsurf | Host::Cline => {
                Shape::McpServers
            }
            Host::Copilot => Shape::VsCode,
            Host::OpenCode => Shape::OpenCode,
            Host::Codex => Shape::Toml,
        }
    }

    /// The file that holds this host's MCP registration.
    pub fn config_path(self, layout: &Layout) -> PathBuf {
        let home = &layout.home;
        match self {
            // Project scope. `~/.claude.json` is Claude's live session state,
            // and merging into a file the app rewrites underneath us would lose
            // whichever side wrote last.
            Host::Claude => layout.repo_root.join(".mcp.json"),
            Host::Cursor => home.join(".cursor").join("mcp.json"),
            Host::Codex => home.join(".codex").join("config.toml"),
            Host::Gemini => home.join(".gemini").join("settings.json"),
            Host::Copilot => layout.repo_root.join(".vscode").join("mcp.json"),
            // The directory kept its pre-rebrand Codeium name.
            Host::Windsurf => home
                .join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"),
            Host::Cline => cline_settings_path(home),
            Host::OpenCode => home.join(".config").join("opencode").join("opencode.json"),
        }
    }

    /// The instruction file that carries the graph section, where the host has
    /// one. Cursor is absent deliberately: it reads `.cursor/rules/*.mdc`
    /// rather than a single file, and TOK does not own that directory.
    pub fn instructions_path(self, layout: &Layout) -> Option<PathBuf> {
        let root = &layout.repo_root;
        Some(match self {
            Host::Claude => root.join("CLAUDE.md"),
            Host::Codex | Host::OpenCode => root.join("AGENTS.md"),
            Host::Gemini => root.join("GEMINI.md"),
            Host::Copilot => root.join(".github").join("copilot-instructions.md"),
            Host::Windsurf => root.join(".windsurfrules"),
            Host::Cline => root.join(".clinerules"),
            Host::Cursor => return None,
        })
    }
}

/// Cline stores its servers in VS Code's extension storage, which moves by OS.
fn cline_settings_path(home: &Path) -> PathBuf {
    let base = if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("Code")
    } else if cfg!(target_os = "windows") {
        home.join("AppData").join("Roaming").join("Code")
    } else {
        home.join(".config").join("Code")
    };

    base.join("User")
        .join("globalStorage")
        .join("saoudrizwan.claude-dev")
        .join("settings")
        .join("cline_mcp_settings.json")
}

/// The command agents should run to reach this server.
///
/// An absolute path, because an editor started from a desktop icon inherits a
/// minimal `PATH` that often lacks `~/.cargo/bin` — the registration then works
/// from a terminal and silently fails in the GUI.
pub fn server_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_else(|| "tok".to_string())
}

/// Wire the graph into a set of hosts: register the MCP server and write the
/// instruction section.
///
/// One host failing does not stop the others. A missing Windsurf directory
/// should not cost someone their Claude registration, and the summary says
/// which host had the problem so it can be fixed on its own.
pub fn wire(hosts: &[Host], layout: &Layout, verbose: u8) -> Result<()> {
    let command = server_command();

    for host in hosts {
        match register(*host, layout, &command) {
            Ok(Outcome::AlreadyPresent) => {
                if verbose > 0 {
                    eprintln!("{}: MCP server already registered", host.label());
                }
            }
            Ok(outcome) => {
                let verb = if outcome == Outcome::Updated {
                    "updated"
                } else {
                    "registered"
                };
                eprintln!(
                    "{}: MCP server {verb} in {}",
                    host.label(),
                    host.config_path(layout).display()
                );
            }
            Err(error) => eprintln!("{}: MCP registration skipped — {error}", host.label()),
        }

        let Some(path) = host.instructions_path(layout) else {
            continue;
        };

        match write_instructions(&path) {
            Ok(Outcome::AlreadyPresent) => {}
            Ok(_) => eprintln!(
                "{}: graph section written to {}",
                host.label(),
                path.display()
            ),
            Err(error) => eprintln!("{}: instructions skipped — {error}", host.label()),
        }
    }

    Ok(())
}

/// Remove what `wire` added, for the same set of hosts.
pub fn unwire(hosts: &[Host], layout: &Layout, verbose: u8) -> Result<()> {
    for host in hosts {
        match unregister(*host, layout) {
            Ok(true) => eprintln!("{}: MCP server removed", host.label()),
            Ok(false) => {
                if verbose > 0 {
                    eprintln!("{}: no MCP registration to remove", host.label());
                }
            }
            Err(error) => eprintln!("{}: MCP removal skipped — {error}", host.label()),
        }

        let Some(path) = host.instructions_path(layout) else {
            continue;
        };

        match remove_instructions(&path) {
            Ok(true) => eprintln!(
                "{}: graph section removed from {}",
                host.label(),
                path.display()
            ),
            Ok(false) => {}
            Err(error) => eprintln!("{}: instruction removal skipped — {error}", host.label()),
        }
    }

    Ok(())
}

/// Register the MCP server with one host, creating the config if needed.
pub fn register(host: Host, layout: &Layout, command: &str) -> Result<Outcome> {
    let path = host.config_path(layout);

    match host.shape() {
        Shape::Toml => register_toml(&path, command),
        shape => register_json(&path, shape, command),
    }
}

/// Remove the registration, leaving every other server in place.
///
/// Returns whether anything was actually removed.
pub fn unregister(host: Host, layout: &Layout) -> Result<bool> {
    let path = host.config_path(layout);
    if !path.exists() {
        return Ok(false);
    }

    if host.shape() == Shape::Toml {
        return unregister_toml(&path);
    }

    let mut root = read_json(&path)?;
    let Some(table) = server_table_mut(&mut root, host.shape()) else {
        return Ok(false);
    };

    let removed = table.remove(SERVER_NAME).is_some();
    if removed {
        write_json(&path, &root)?;
    }

    Ok(removed)
}

fn register_json(path: &Path, shape: Shape, command: &str) -> Result<Outcome> {
    let mut root = if path.exists() {
        read_json(path)?
    } else {
        json!({})
    };

    let entry = entry_for(shape, command);

    let table = server_table_mut(&mut root, shape)
        .context("Config has a non-object where the MCP servers belong")?;

    let outcome = match table.get(SERVER_NAME) {
        Some(existing) if *existing == entry => return Ok(Outcome::AlreadyPresent),
        Some(_) => Outcome::Updated,
        None => Outcome::Added,
    };

    table.insert(SERVER_NAME.to_string(), entry);
    write_json(path, &root)?;

    Ok(outcome)
}

/// The server entry, in whichever dialect the host speaks.
fn entry_for(shape: Shape, command: &str) -> Value {
    match shape {
        Shape::McpServers => json!({ "command": command, "args": ["mcp"] }),
        Shape::VsCode => json!({ "type": "stdio", "command": command, "args": ["mcp"] }),
        // One array, and no `enabled` key: current OpenCode connects every
        // server that is not explicitly disabled.
        Shape::OpenCode => json!({ "type": "local", "command": [command, "mcp"] }),
        Shape::Toml => unreachable!("TOML is written textually, not as JSON"),
    }
}

/// Borrow (creating if absent) the object that holds the server entries.
fn server_table_mut(root: &mut Value, shape: Shape) -> Option<&mut serde_json::Map<String, Value>> {
    let path: &[&str] = match shape {
        Shape::McpServers => &["mcpServers"],
        Shape::VsCode => &["servers"],
        Shape::OpenCode => &["mcp", "servers"],
        Shape::Toml => return None,
    };

    let mut node = root;
    for key in path {
        if !node.get(*key).is_some_and(Value::is_object) {
            node.as_object_mut()?.insert((*key).to_string(), json!({}));
        }
        node = node.get_mut(*key)?;
    }

    node.as_object_mut()
}

/// Codex config is TOML, and round-tripping it through a value tree would strip
/// the comments people keep in it. Appending the block preserves the file
/// exactly, at the cost of only being able to add or remove it whole.
fn register_toml(path: &Path, command: &str) -> Result<Outcome> {
    let existing = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?
    } else {
        String::new()
    };

    let block = format!(
        "[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [\"mcp\"]\n",
        toml_string(command)
    );

    if let Some(range) = toml_block_range(&existing) {
        if existing[range.clone()].trim() == block.trim() {
            return Ok(Outcome::AlreadyPresent);
        }

        let mut updated = existing.clone();
        updated.replace_range(range, &block);
        write_atomic(path, &updated)?;
        return Ok(Outcome::Updated);
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(&block);

    write_atomic(path, &updated)?;
    Ok(Outcome::Added)
}

fn unregister_toml(path: &Path) -> Result<bool> {
    let existing =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let Some(range) = toml_block_range(&existing) else {
        return Ok(false);
    };

    let mut updated = existing;
    updated.replace_range(range, "");
    write_atomic(path, updated.trim_end())?;

    Ok(true)
}

/// Byte range of the `[mcp_servers.tok]` block: the header through to the line
/// before the next table header, or the end of the file.
fn toml_block_range(contents: &str) -> Option<std::ops::Range<usize>> {
    let header = format!("[mcp_servers.{SERVER_NAME}]");
    let start = contents
        .lines()
        .scan(0usize, |offset, line| {
            let at = *offset;
            *offset += line.len() + 1;
            Some((at, line))
        })
        .find(|(_, line)| line.trim() == header)
        .map(|(at, _)| at)?;

    let end = contents[start..]
        .match_indices('\n')
        .map(|(index, _)| start + index + 1)
        .find(|&at| {
            contents[at..]
                .lines()
                .next()
                .is_some_and(|line| line.trim_start().starts_with('['))
        })
        .unwrap_or(contents.len());

    Some(start..end)
}

/// Quote a value for TOML, escaping what would otherwise break the string.
fn toml_string(value: &str) -> String {
    let escaped = value.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

/// Add or refresh the graph section in an instruction file, leaving everything
/// outside the markers untouched.
pub fn write_instructions(path: &Path) -> Result<Outcome> {
    let existing = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?
    } else {
        String::new()
    };

    let (updated, outcome) = upsert_section(&existing, GRAPH_INSTRUCTIONS);
    if outcome == Outcome::AlreadyPresent {
        return Ok(outcome);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    write_atomic(path, &updated)?;

    Ok(outcome)
}

/// Replace the marked section if present, otherwise append it.
fn upsert_section(existing: &str, section: &str) -> (String, Outcome) {
    let bounds = existing.find(SECTION_START).and_then(|start| {
        existing[start..]
            .find(SECTION_END)
            .map(|end| (start, start + end + SECTION_END.len()))
    });

    let Some((start, end)) = bounds else {
        let mut updated = existing.to_string();
        if !updated.is_empty() {
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push('\n');
        }
        updated.push_str(section);
        updated.push('\n');
        return (updated, Outcome::Added);
    };

    if existing[start..end] == *section {
        return (existing.to_string(), Outcome::AlreadyPresent);
    }

    let mut updated = existing.to_string();
    updated.replace_range(start..end, section);
    (updated, Outcome::Updated)
}

/// Remove the graph section, leaving the rest of the file alone.
pub fn remove_instructions(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let existing =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let Some(start) = existing.find(SECTION_START) else {
        return Ok(false);
    };
    // Without a closing marker there is no way to tell where TOK's text stops
    // and the user's begins, so removing nothing is the safe answer.
    let Some(end) = existing[start..].find(SECTION_END) else {
        return Ok(false);
    };

    let mut updated = existing.clone();
    updated.replace_range(start..start + end + SECTION_END.len(), "");
    write_atomic(path, updated.trim_end())?;

    Ok(true)
}

fn read_json(path: &Path) -> Result<Value> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    if contents.trim().is_empty() {
        return Ok(json!({}));
    }

    serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse {} as JSON", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut rendered = serde_json::to_string_pretty(value).context("Failed to render JSON")?;
    rendered.push('\n');
    write_atomic(path, &rendered)
}

/// Write through a temporary file in the same directory.
///
/// These are config files an editor may be reading at any moment; a truncated
/// one is silently ignored by most hosts, which presents as "MCP just stopped
/// working" with no error anywhere.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let temp = path.with_extension("tok-tmp");
    fs::write(&temp, contents).with_context(|| format!("Failed to write {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("Failed to replace {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout = Layout {
            home: dir.path().join("home"),
            repo_root: dir.path().join("repo"),
        };
        (dir, layout)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("read")
    }

    fn json_at(path: &Path) -> Value {
        serde_json::from_str(&read(path)).expect("valid json")
    }

    // --------------------------------------------------------- registration

    #[test]
    fn every_host_registers_into_its_own_file() {
        let (_dir, layout) = layout();

        for host in Host::all() {
            let outcome = register(host, &layout, "/bin/tok")
                .unwrap_or_else(|e| panic!("{}: {e:#}", host.label()));

            assert_eq!(outcome, Outcome::Added, "{}", host.label());
            assert!(
                host.config_path(&layout).exists(),
                "{} wrote nothing",
                host.label()
            );
        }
    }

    #[test]
    fn no_two_hosts_share_a_config_file() {
        let (_dir, layout) = layout();

        let mut seen = std::collections::HashSet::new();
        for host in Host::all() {
            let path = host.config_path(&layout);
            assert!(seen.insert(path.clone()), "{} collides", host.label());
        }
    }

    #[test]
    fn the_standard_shape_uses_command_and_args() {
        let (_dir, layout) = layout();

        register(Host::Cursor, &layout, "/bin/tok").expect("register");
        let config = json_at(&Host::Cursor.config_path(&layout));

        assert_eq!(config["mcpServers"]["tok"]["command"], "/bin/tok");
        assert_eq!(config["mcpServers"]["tok"]["args"][0], "mcp");
    }

    /// VS Code needs the transport spelled out; without it the server is
    /// listed and never started.
    #[test]
    fn vs_code_gets_an_explicit_stdio_transport() {
        let (_dir, layout) = layout();

        register(Host::Copilot, &layout, "/bin/tok").expect("register");
        let config = json_at(&Host::Copilot.config_path(&layout));

        assert_eq!(config["servers"]["tok"]["type"], "stdio");
        assert_eq!(config["servers"]["tok"]["command"], "/bin/tok");
    }

    /// OpenCode takes the executable and its arguments as a single array,
    /// nested under `mcp.servers`.
    #[test]
    fn opencode_gets_one_command_array() {
        let (_dir, layout) = layout();

        register(Host::OpenCode, &layout, "/bin/tok").expect("register");
        let config = json_at(&Host::OpenCode.config_path(&layout));
        let entry = &config["mcp"]["servers"]["tok"];

        assert_eq!(entry["type"], "local");
        assert_eq!(entry["command"], json!(["/bin/tok", "mcp"]));
        assert!(entry.get("args").is_none());
    }

    #[test]
    fn codex_is_written_as_toml() {
        let (_dir, layout) = layout();

        register(Host::Codex, &layout, "/bin/tok").expect("register");
        let contents = read(&Host::Codex.config_path(&layout));

        assert!(contents.contains("[mcp_servers.tok]"));
        assert!(contents.contains(r#"command = "/bin/tok""#));
        assert!(
            toml::from_str::<toml::Value>(&contents).is_ok(),
            "invalid TOML:\n{contents}"
        );
    }

    // ------------------------------------------------------------ idempotency

    #[test]
    fn registering_twice_changes_nothing_the_second_time() {
        let (_dir, layout) = layout();

        for host in Host::all() {
            register(host, &layout, "/bin/tok").expect("first");
            let after_first = read(&host.config_path(&layout));

            let outcome = register(host, &layout, "/bin/tok").expect("second");

            assert_eq!(outcome, Outcome::AlreadyPresent, "{}", host.label());
            assert_eq!(
                read(&host.config_path(&layout)),
                after_first,
                "{} rewrote the file",
                host.label()
            );
        }
    }

    /// Reinstalling to a new location has to move the registration with it.
    #[test]
    fn a_changed_command_updates_the_registration() {
        let (_dir, layout) = layout();

        for host in Host::all() {
            register(host, &layout, "/old/tok").expect("first");

            let outcome = register(host, &layout, "/new/tok").expect("second");

            assert_eq!(outcome, Outcome::Updated, "{}", host.label());
            let contents = read(&host.config_path(&layout));
            assert!(contents.contains("/new/tok"), "{}", host.label());
            assert!(!contents.contains("/old/tok"), "{}", host.label());
        }
    }

    // ------------------------------------------------------- preserving state

    /// These files hold the user's other servers. Losing them would be worse
    /// than never registering at all.
    #[test]
    fn other_servers_survive_registration() {
        let (_dir, layout) = layout();
        let path = Host::Cursor.config_path(&layout);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(
            &path,
            r#"{"mcpServers":{"other":{"command":"other","args":[]}}}"#,
        )
        .expect("seed");

        register(Host::Cursor, &layout, "/bin/tok").expect("register");

        let config = json_at(&path);
        assert_eq!(config["mcpServers"]["other"]["command"], "other");
        assert_eq!(config["mcpServers"]["tok"]["command"], "/bin/tok");
    }

    #[test]
    fn unrelated_settings_survive_registration() {
        let (_dir, layout) = layout();
        let path = Host::Gemini.config_path(&layout);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, r#"{"theme":"dark","telemetry":{"enabled":false}}"#).expect("seed");

        register(Host::Gemini, &layout, "/bin/tok").expect("register");

        let config = json_at(&path);
        assert_eq!(config["theme"], "dark");
        assert_eq!(config["telemetry"]["enabled"], false);
    }

    /// Codex config is hand-edited and full of comments, which is why the
    /// block is appended textually rather than round-tripped.
    #[test]
    fn codex_comments_and_settings_survive_registration() {
        let (_dir, layout) = layout();
        let path = Host::Codex.config_path(&layout);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let original = "# my notes\nmodel = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        fs::write(&path, original).expect("seed");

        register(Host::Codex, &layout, "/bin/tok").expect("register");

        let contents = read(&path);
        assert!(contents.contains("# my notes"), "comment lost:\n{contents}");
        assert!(contents.contains("[mcp_servers.other]"));
        assert!(contents.contains("[mcp_servers.tok]"));
        assert!(toml::from_str::<toml::Value>(&contents).is_ok());
    }

    #[test]
    fn an_empty_config_file_is_treated_as_empty_json() {
        let (_dir, layout) = layout();
        let path = Host::Cursor.config_path(&layout);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, "   \n").expect("seed");

        register(Host::Cursor, &layout, "/bin/tok").expect("register");

        assert_eq!(json_at(&path)["mcpServers"]["tok"]["command"], "/bin/tok");
    }

    /// Overwriting a config we cannot parse would discard whatever is in it.
    #[test]
    fn a_corrupt_config_is_reported_rather_than_overwritten() {
        let (_dir, layout) = layout();
        let path = Host::Cursor.config_path(&layout);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, "{ not json").expect("seed");

        let error = register(Host::Cursor, &layout, "/bin/tok").expect_err("should fail");

        assert!(format!("{error}").contains("JSON"));
        assert_eq!(read(&path), "{ not json");
    }

    // ------------------------------------------------------------- unregister

    #[test]
    fn unregister_removes_only_the_tok_entry() {
        let (_dir, layout) = layout();
        let path = Host::Cursor.config_path(&layout);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, r#"{"mcpServers":{"other":{"command":"other"}}}"#).expect("seed");
        register(Host::Cursor, &layout, "/bin/tok").expect("register");

        assert!(unregister(Host::Cursor, &layout).expect("unregister"));

        let config = json_at(&path);
        assert!(config["mcpServers"].get("tok").is_none());
        assert_eq!(config["mcpServers"]["other"]["command"], "other");
    }

    #[test]
    fn every_host_can_be_unregistered() {
        let (_dir, layout) = layout();

        for host in Host::all() {
            register(host, &layout, "/bin/tok").expect("register");

            assert!(
                unregister(host, &layout).expect("unregister"),
                "{}",
                host.label()
            );
            assert!(
                !read(&host.config_path(&layout)).contains("/bin/tok"),
                "{} kept the entry",
                host.label()
            );
        }
    }

    #[test]
    fn unregistering_what_was_never_registered_is_not_an_error() {
        let (_dir, layout) = layout();

        for host in Host::all() {
            assert!(
                !unregister(host, &layout).expect("unregister"),
                "{}",
                host.label()
            );
        }
    }

    #[test]
    fn codex_unregister_keeps_the_other_tables() {
        let (_dir, layout) = layout();
        let path = Host::Codex.config_path(&layout);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(
            &path,
            "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other\"\n",
        )
        .expect("seed");
        register(Host::Codex, &layout, "/bin/tok").expect("register");

        assert!(unregister(Host::Codex, &layout).expect("unregister"));

        let contents = read(&path);
        assert!(!contents.contains("[mcp_servers.tok]"), "{contents}");
        assert!(contents.contains("[mcp_servers.other]"));
        assert!(contents.contains("model = \"o3\""));
        assert!(
            toml::from_str::<toml::Value>(&contents).is_ok(),
            "{contents}"
        );
    }

    /// A block in the middle of the file must not swallow what follows it.
    #[test]
    fn a_codex_block_is_bounded_by_the_next_table() {
        let contents = "[mcp_servers.tok]\ncommand = \"x\"\n\n[other]\nkeep = true\n";

        let range = toml_block_range(contents).expect("range");

        assert!(contents[range.clone()].contains("command = \"x\""));
        assert!(!contents[range].contains("[other]"));
    }

    // ----------------------------------------------------------- instructions

    #[test]
    fn instructions_are_appended_to_a_new_file() {
        let (dir, _layout) = layout();
        let path = dir.path().join("AGENTS.md");

        let outcome = write_instructions(&path).expect("write");

        assert_eq!(outcome, Outcome::Added);
        let contents = read(&path);
        assert!(contents.contains("## Code graph (TOK)"));
        assert!(contents.contains("tok mem ask"));
    }

    #[test]
    fn existing_instructions_are_preserved_around_the_section() {
        let (dir, _layout) = layout();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, "# House rules\n\nAlways run the tests.\n").expect("seed");

        write_instructions(&path).expect("write");

        let contents = read(&path);
        assert!(contents.contains("# House rules"));
        assert!(contents.contains("Always run the tests."));
        assert!(contents.contains("## Code graph (TOK)"));
    }

    #[test]
    fn rewriting_instructions_is_idempotent() {
        let (dir, _layout) = layout();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, "# House rules\n").expect("seed");
        write_instructions(&path).expect("first");
        let after_first = read(&path);

        let outcome = write_instructions(&path).expect("second");

        assert_eq!(outcome, Outcome::AlreadyPresent);
        assert_eq!(read(&path), after_first);
    }

    /// An upgraded TOK replaces its own section in place, rather than stacking
    /// a second copy under the first.
    #[test]
    fn a_stale_section_is_replaced_not_duplicated() {
        let (dir, _layout) = layout();
        let path = dir.path().join("CLAUDE.md");
        fs::write(
            &path,
            "# Rules\n\n<!-- tok-graph -->\nold guidance\n<!-- /tok-graph -->\n\n## After\n",
        )
        .expect("seed");

        let outcome = write_instructions(&path).expect("write");

        assert_eq!(outcome, Outcome::Updated);
        let contents = read(&path);
        assert_eq!(contents.matches(SECTION_START).count(), 1);
        assert!(!contents.contains("old guidance"));
        assert!(contents.contains("# Rules"));
        assert!(contents.contains("## After"));
    }

    #[test]
    fn removing_instructions_leaves_the_rest_of_the_file() {
        let (dir, _layout) = layout();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, "# Rules\n\nkeep me\n").expect("seed");
        write_instructions(&path).expect("write");

        assert!(remove_instructions(&path).expect("remove"));

        let contents = read(&path);
        assert!(contents.contains("keep me"));
        assert!(!contents.contains("Code graph (TOK)"));
    }

    /// Without a closing marker there is no way to know where TOK's text ends,
    /// so nothing is removed.
    #[test]
    fn a_damaged_section_is_left_alone() {
        let (dir, _layout) = layout();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, "# Rules\n<!-- tok-graph -->\nhalf a section\n").expect("seed");

        assert!(!remove_instructions(&path).expect("remove"));
        assert!(read(&path).contains("half a section"));
    }

    #[test]
    fn every_host_but_cursor_has_an_instruction_file() {
        let (_dir, layout) = layout();

        for host in Host::all() {
            let expected = host != Host::Cursor;
            assert_eq!(
                host.instructions_path(&layout).is_some(),
                expected,
                "{}",
                host.label()
            );
        }
    }

    // ------------------------------------------------------------------ misc

    #[test]
    fn every_host_has_a_distinct_label() {
        let labels: std::collections::HashSet<&str> =
            Host::all().iter().map(|host| host.label()).collect();

        assert_eq!(labels.len(), Host::all().len());
    }

    #[test]
    fn windows_paths_are_escaped_for_toml() {
        assert_eq!(toml_string(r"C:\bin\tok.exe"), r#""C:\\bin\\tok.exe""#);
    }

    #[test]
    fn a_registered_command_is_an_absolute_path_when_one_is_available() {
        let command = server_command();

        assert!(
            Path::new(&command).is_absolute() || command == "tok",
            "unexpected command: {command}"
        );
    }
}
